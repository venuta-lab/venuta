import { motion } from 'motion/react'
import { useEffect, useMemo, useState } from 'react'
import { askAddSongs } from '../../components/AddToPlaylist'
import { AvailabilityBadge } from '../../components/AvailabilityBadge'
import { IconPause, IconPlay, IconPlaylistAdd, IconShuffle } from '../../components/Icons'
import { RemoteArt, cleanArtUrl } from '../../components/RemoteArt'
import { Detail } from '../../components/ui'
import { onlineAlbumTracks, type OnlineAlbum, type OnlineTrack } from '../../lib/engine'
import { onlineProblem } from '../../lib/online'
import { current, enqueueItems, fromIs, onlineEntryFrom, playAllItems, playItems, queueKey, toRemoteItem, toggle, usePlayerValue, type From } from '../../lib/player'
import { minutes, plural, spring, useNav } from './model'
import { OnlineTrackHeader, OnlineTrackRow } from './OnlineResults'

/*
  An online catalog album as its own page, the way a library album has one: its picture, what
  it is, Play / Shuffle / Add to playlist, and its songs in the same columns as Online search.
  Back returns to the search it was opened from.
*/

const keyOf = (t: OnlineTrack) => `online:${t.provider}:${t.track_id}`

export function OnlineAlbumPage({ album: a, onBack }: { album: OnlineAlbum; onBack: () => void }) {
  const { root } = useNav()
  const [tracks, setTracks] = useState<OnlineTrack[] | null>(null)
  const [problem, setProblem] = useState('')
  useEffect(() => {
    let live = true
    onlineAlbumTracks(a.album_id)
      .then((t) => live && setTracks(t))
      .catch((e) => {
        if (!live) return
        setProblem(onlineProblem(e))
        setTracks([])
      })
    return () => {
      live = false
    }
  }, [a.album_id])

  const items = useMemo(() => (tracks ?? []).map(toRemoteItem), [tracks])
  // "Playing from" names the album; choosing it there searches for it again.
  const from: From = useMemo(() => ({ kind: 'search', ref: a.title, label: `Online: ${a.title}` }), [a.title])
  const nowKey = usePlayerValue((s) => {
    const c = current(s)
    return c ? queueKey(c) : ''
  })
  const playing = usePlayerValue((s) => s.playing)
  const nowFrom = usePlayerValue((s) => s.from)
  const isNow = fromIs(nowFrom, 'search', a.title) && (tracks ?? []).some((t) => keyOf(t) === nowKey)
  const art = cleanArtUrl(a.artwork_url)
  const total = (tracks ?? []).reduce((n, t) => n + (t.duration_ms ?? 0), 0)
  const year = a.release_date?.slice(0, 4)
  const addAll = () => tracks?.length && askAddSongs(tracks.map((t) => ({ online: onlineEntryFrom(t) })), a.title)

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
    <Detail title={a.title} onBack={onBack} glow={art} tools={items.length ? playButton('sm') : undefined}>
      <div className="relative flex flex-wrap items-end gap-8">
        <div className="grid aspect-square w-[240px] shrink-0 place-items-center overflow-hidden rounded-3xl bg-ink/[0.06] text-[56px] text-ink/25 shadow-2xl shadow-black/40">
          <RemoteArt url={a.artwork_url} seed={a.title} imgClassName="h-full w-full object-cover" />
        </div>
        <motion.div className="min-w-[260px] flex-1 pb-1" initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ ...spring, delay: 0.05 }}>
          <p className="flex items-center gap-2 text-[13px] font-medium text-ink/60">
            Album <AvailabilityBadge availability="online" />
          </p>
          <h1 className="title hero-title mt-1 text-[34px] leading-[1.1]">{a.title}</h1>
          {a.artists.length > 0 && <p className="mt-2 text-[16px] font-medium text-ink/85">{a.artists.join(', ')}</p>}
          <p className="mt-2.5 text-[13px] text-ink/60">
            {[year, tracks === null ? 'Reading its songs…' : tracks.length ? `${plural(tracks.length, 'song')}, ${minutes(total)}` : ''].filter(Boolean).join(' · ')}
          </p>
          <div className="mt-5 flex flex-wrap items-center gap-2.5">
            {playButton('lg')}
            <button disabled={items.length < 2} onClick={() => void playAllItems(root, items, from, 'shuffle')} className="btn btn-lg shuffle-main">
              <IconShuffle />
              Shuffle
            </button>
            <button disabled={!items.length} onClick={addAll} className="btn-ghost btn-lg">
              <IconPlaylistAdd width={18} height={18} />
              Add to playlist
            </button>
          </div>
        </motion.div>
      </div>

      <div className="mt-9">
        {problem ? (
          <p role="status" className="py-6 text-[13px] text-ink/55">
            {problem}
          </p>
        ) : tracks === null ? (
          <p className="py-6 text-[13px] text-ink/45">Reading the album’s songs from the online catalog…</p>
        ) : !tracks.length ? (
          <p className="py-6 text-[13px] text-ink/55">The online catalog lists no songs for this album.</p>
        ) : (
          <>
            <OnlineTrackHeader />
            <div className="mt-1 grid gap-0.5">
              {tracks.map((t, i) => (
                <OnlineTrackRow
                  key={keyOf(t)}
                  track={t}
                  now={isNow && keyOf(t) === nowKey}
                  onPlay={() => (isNow && keyOf(t) === nowKey ? void toggle() : void playItems(root, items, i, from))}
                  onQueue={(next) => void enqueueItems(root, [toRemoteItem(t)], next)}
                  onAdd={() => askAddSongs([{ online: onlineEntryFrom(t) }], t.title)}
                />
              ))}
            </div>
          </>
        )}
      </div>
    </Detail>
  )
}
