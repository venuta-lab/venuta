import { motion, Reorder, useDragControls } from 'motion/react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { askAddSongs } from '../../components/AddToPlaylist'
import { AvailabilityBadge } from '../../components/AvailabilityBadge'
import { confirmAsk } from '../../components/Confirm'
import { DragHandle } from '../../components/DragHandle'
import { IconClose, IconPause, IconPlay, IconPlaylistAdd, IconShuffle } from '../../components/Icons'
import { RemoteArt, artUrl } from '../../components/RemoteArt'
import { Detail } from '../../components/ui'
import { SavePlaylistButton } from '../../components/YtmButtons'
import { onlineCollection, ytmCloseToCollaborators, ytmInvite, ytmPlaylist, ytmPlaylistDelete, ytmPlaylistEdit, type OnlineCollection, type OnlineTrack, type YtmEdit, type YtmPlaylist, type YtmTrack } from '../../lib/engine'
import { notify } from '../../lib/notify'
import { onlineProblem } from '../../lib/online'
import { current, enqueueItems, fromIs, onlineEntryFrom, playAllItems, playItems, queueKey, toRemoteItem, toggle, usePlayerValue, type From } from '../../lib/player'
import { useYtmStatus, ytmLibraryChanged } from '../../lib/ytmLibrary'
import { movesBetween } from '../../lib/ytmMoves'
import { minutes, plural, spring, useNav } from './model'
import { OnlineTrackHeader, OnlineTrackRow } from './OnlineResults'

/*
  An online playlist or podcast as its own page, made like an online album's: its cover, what it
  is and whose, Play / Shuffle / Add to playlist, then its songs (or episodes) in the columns of
  Online search. Back returns to where it was opened.

  Signed in, a playlist is read as your account sees it. One of yours can be changed here: its
  name, words and privacy, its order by dragging, a song taken out, the whole playlist deleted
  (asked first). Someone else's can be saved to your library.
*/

const keyOf = (t: OnlineTrack) => `online:${t.provider}:${t.track_id}`
const setIds = (ts: YtmTrack[]) => ts.flatMap((t) => (t.set_video_id ? [t.set_video_id] : []))

const PRIVACY: [string, string][] = [
  ['PRIVATE', 'Private'],
  ['UNLISTED', 'Unlisted'],
  ['PUBLIC', 'Public'],
]

export function OnlineCollectionPage({ collection: c, onBack }: { collection: OnlineCollection; onBack: () => void }) {
  const { root } = useNav()
  const { signed_in } = useYtmStatus()
  const [tracks, setTracks] = useState<YtmTrack[] | null>(null)
  // The playlist as the account sees it (signed in): whether it's yours, and each song's place.
  const [mine, setMine] = useState<YtmPlaylist | null>(null)
  const [problem, setProblem] = useState('')
  const [stamp, setStamp] = useState(0)
  // The order YouTube Music last had (its items' set ids): what a drag is measured against.
  const sent = useRef<string[]>([])
  const asAccount = signed_in && c.kind === 'playlist'
  useEffect(() => {
    let live = true
    const plain = () =>
      onlineCollection(c.collection_id)
        .then((p) => live && setTracks(p.tracks))
        .catch((e) => {
          if (!live) return
          setProblem(onlineProblem(e))
          setTracks([])
        })
    if (asAccount)
      ytmPlaylist(c.collection_id.replace(/^VL/, ''))
        .then((p) => {
          if (!live) return
          setMine(p)
          setTracks(p.tracks)
          sent.current = setIds(p.tracks)
        })
        .catch(() => live && void plain())
    else void plain()
    return () => {
      live = false
    }
  }, [c.collection_id, asAccount, stamp])
  const owned = !!mine?.owned
  // Open to collaborators: anyone in it adds songs, and each takes out only what they added.
  const collab = !!mine?.collaborative
  const playlistId = mine?.playlist_id ?? c.collection_id.replace(/^VL/, '')
  const title = mine?.title || c.title

  const items = useMemo(() => (tracks ?? []).map(toRemoteItem), [tracks])
  const from: From = useMemo(() => ({ kind: 'search', ref: c.title, label: `Online: ${c.title}` }), [c.title])
  const nowKey = usePlayerValue((s) => {
    const now = current(s)
    return now ? queueKey(now) : ''
  })
  const playing = usePlayerValue((s) => s.playing)
  const nowFrom = usePlayerValue((s) => s.from)
  const isNow = fromIs(nowFrom, 'search', c.title) && (tracks ?? []).some((t) => keyOf(t) === nowKey)
  const total = (tracks ?? []).reduce((n, t) => n + (t.duration_ms ?? 0), 0)
  const podcast = c.kind === 'podcast'
  const unit = podcast ? 'episode' : 'song'
  const addAll = () => tracks?.length && askAddSongs(tracks.map((t) => ({ online: onlineEntryFrom(t) })), title)

  /** Send changes; on a refusal the page is read again, so it never shows what didn't happen. */
  const send = async (edits: YtmEdit[], done?: string) => {
    try {
      await ytmPlaylistEdit(playlistId, edits)
      ytmLibraryChanged(['playlists'])
      if (done) notify(done)
    } catch (e) {
      notify(onlineProblem(e))
      setStamp((n) => n + 1)
    }
  }
  const remove = (t: YtmTrack) => {
    if (!t.set_video_id || !tracks) return
    const left = tracks.filter((x) => x !== t)
    setTracks(left)
    sent.current = setIds(left)
    void send([{ kind: 'remove', items: [[t.track_id, t.set_video_id]] }], `Took “${t.title}” out of “${title}”.`)
  }
  const reorder = (next: YtmTrack[]) => {
    const after = setIds(next)
    const moves = movesBetween(sent.current, after)
    if (!moves.length) return
    sent.current = after
    void send(moves.map((m) => ({ kind: 'move', set_video_id: m.set_video_id, before: m.before })))
  }
  const remove_all = async () => {
    const ok = await confirmAsk({
      title: `Delete “${title}” on YouTube Music?`,
      text: 'The playlist goes from your YouTube Music account, on every device. Songs saved elsewhere stay. This can’t be undone from Venuta.',
      confirm: 'Delete playlist',
      danger: true,
    })
    if (!ok) return
    try {
      await ytmPlaylistDelete(playlistId)
      ytmLibraryChanged(['playlists'])
      notify(`Deleted “${title}” on YouTube Music.`)
      onBack()
    } catch (e) {
      notify(onlineProblem(e))
    }
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

  const row = (t: YtmTrack, i: number) => (
    <OnlineTrackRow
      track={t}
      now={isNow && keyOf(t) === nowKey}
      onPlay={() => (isNow && keyOf(t) === nowKey ? void toggle() : void playItems(root, items, i, from))}
      onQueue={(next) => void enqueueItems(root, [toRemoteItem(t)], next)}
      onAdd={() => askAddSongs([{ online: onlineEntryFrom(t) }], t.title)}
    />
  )

  return (
    <Detail title={title} onBack={onBack} glow={artUrl(c.artwork_url, 240)} tools={items.length ? playButton('sm') : undefined}>
      <div className="relative flex flex-wrap items-end gap-8">
        <div className="grid aspect-square w-[240px] shrink-0 place-items-center overflow-hidden rounded-3xl bg-ink/[0.06] text-[56px] text-ink/25 shadow-2xl shadow-black/40">
          <RemoteArt url={mine?.artwork_url ?? c.artwork_url} seed={title} size={240} imgClassName="h-full w-full object-cover" />
        </div>
        <motion.div className="min-w-[260px] flex-1 pb-1" initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ ...spring, delay: 0.05 }}>
          <p className="flex items-center gap-2 text-[13px] font-medium text-ink/60">
            {podcast ? 'Podcast' : collab ? 'Collaborative playlist' : owned ? 'Your playlist' : 'Playlist'} <AvailabilityBadge availability="online" />
          </p>
          {owned && mine ? (
            <Details mine={mine} onSave={(edits, next) => {
              setMine({ ...mine, ...next })
              void send(edits, 'Saved on YouTube Music.')
            }} />
          ) : (
            <>
              <h1 className="title hero-title mt-1 text-[34px] leading-[1.1]">{title}</h1>
              {c.subtitle && <p className="mt-2 text-[15px] font-medium text-ink/80">{c.subtitle}</p>}
            </>
          )}
          <p className="mt-2.5 text-[13px] text-ink/60">
            {tracks === null ? `Reading its ${unit}s…` : tracks.length ? `${plural(tracks.length, unit)}, ${minutes(total)}` : ''}
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
            {asAccount && mine && !owned && <SavePlaylistButton playlistId={playlistId} />}
            {owned && (
              <button onClick={() => void remove_all()} className="btn-ghost btn-lg">
                Delete playlist
              </button>
            )}
          </div>
          {asAccount && mine && !podcast && (owned || collab) && <Collaborate mine={mine} onChanged={() => setStamp((n) => n + 1)} />}
        </motion.div>
      </div>

      <div className="mt-9">
        {problem ? (
          <p role="status" className="py-6 text-[13px] text-ink/55">
            {problem}
          </p>
        ) : tracks === null ? (
          <p className="py-6 text-[13px] text-ink/45">Reading the {unit}s from the online catalog…</p>
        ) : !tracks.length ? (
          <p className="py-6 text-[13px] text-ink/55">{owned ? 'No songs yet. Add some from any online song’s menu (Add to playlist).' : `The online catalog lists no ${unit}s here.`}</p>
        ) : (
          <>
            <OnlineTrackHeader />
            {owned || collab ? (
              <Reorder.Group axis="y" values={tracks} onReorder={(next) => owned && setTracks(next)} className="mt-1 grid gap-0.5">
                {tracks.map((t, i) => (
                  <OwnedRow key={t.set_video_id ?? `${keyOf(t)}:${i}`} track={t} canDrag={owned} onRemove={() => remove(t)} onDrop={() => owned && reorder(tracks)}>
                    {row(t, i)}
                  </OwnedRow>
                ))}
              </Reorder.Group>
            ) : (
              <div className="mt-1 grid gap-0.5">
                {tracks.map((t, i) => (
                  <div key={`${keyOf(t)}:${i}`}>{row(t, i)}</div>
                ))}
              </div>
            )}
          </>
        )}
      </div>
    </Detail>
  )
}

/** A song of your own playlist: dragged by its grip, taken out with ×. In a collaborative one
 *  you don't own, only ×: YouTube Music lets each person take out what they added. */
function OwnedRow(props: { track: YtmTrack; canDrag: boolean; onRemove: () => void; onDrop: () => void; children: React.ReactNode }) {
  const drag = useDragControls()
  return (
    <Reorder.Item value={props.track} dragListener={false} dragControls={drag} onDragEnd={props.onDrop} className="group flex items-center gap-1">
      {props.canDrag && <DragHandle label={props.track.title} onPointerDown={(e) => drag.start(e)} />}
      <div className="min-w-0 flex-1">{props.children}</div>
      <button
        aria-label={`Take ${props.track.title} out of the playlist`}
        onClick={props.onRemove}
        disabled={!props.track.set_video_id}
        className="grid h-7 w-7 shrink-0 place-items-center rounded-full text-ink/45 opacity-0 hover:bg-ink/10 hover:text-ink focus:opacity-100 group-hover:opacity-100"
      >
        <IconClose width={14} height={14} />
      </button>
    </Reorder.Item>
  )
}

/** Your playlist's name, words and privacy, changed in place. */
function Details(props: { mine: YtmPlaylist; onSave: (edits: YtmEdit[], next: Partial<YtmPlaylist>) => void }) {
  const { mine } = props
  const [editing, setEditing] = useState(false)
  const [name, setName] = useState(mine.title)
  const [words, setWords] = useState(mine.description ?? '')
  const save = () => {
    const edits: YtmEdit[] = []
    const n = name.trim()
    if (n && n !== mine.title) edits.push({ kind: 'rename', title: n })
    if (words.trim() !== (mine.description ?? '').trim()) edits.push({ kind: 'describe', description: words.trim() })
    setEditing(false)
    if (edits.length) props.onSave(edits, { title: n || mine.title, description: words.trim() || null })
  }
  return (
    <div className="mt-1">
      {editing ? (
        <div className="grid max-w-[560px] gap-2">
          <input
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') save()
              if (e.key === 'Escape') setEditing(false)
            }}
            aria-label="Playlist name"
            maxLength={150}
            className="field h-11 rounded-xl px-3 text-[20px] font-semibold"
          />
          <textarea
            value={words}
            onChange={(e) => setWords(e.target.value)}
            aria-label="What it is"
            placeholder="What it is (optional)"
            rows={2}
            className="field rounded-xl px-3 py-2 text-[13.5px]"
          />
          <div className="flex gap-2">
            <button onClick={save} className="btn-primary btn-sm">
              Save
            </button>
            <button onClick={() => setEditing(false)} className="btn-ghost btn-sm">
              Cancel
            </button>
          </div>
        </div>
      ) : (
        <>
          <h1 className="title hero-title text-[34px] leading-[1.1]">
            <button onClick={() => setEditing(true)} title="Rename, or change what it says" className="rounded-md text-left hover:underline decoration-ink/30 underline-offset-8">
              {mine.title}
            </button>
          </h1>
          {mine.description && <p className="mt-2 max-w-[68ch] whitespace-pre-line text-[13.5px] leading-relaxed text-ink/70">{mine.description}</p>}
        </>
      )}
      <label className="mt-3 flex items-center gap-2 text-[13px] text-ink/65">
        Who can see it
        <select
          value={mine.privacy ?? 'PRIVATE'}
          onChange={(e) => props.onSave([{ kind: 'privacy', privacy: e.target.value }], { privacy: e.target.value })}
          className="field h-8 rounded-full px-3 text-[13px]"
        >
          {PRIVACY.map(([v, l]) => (
            <option key={v} value={v}>
              {l}
            </option>
          ))}
        </select>
      </label>
    </div>
  )
}

/** Collaborating: the people in it, the invite link, and whether new people may join. Inviting
 *  and closing are the owner's; anyone else is told so by YouTube Music's answer. */
function Collaborate({ mine, onChanged }: { mine: YtmPlaylist; onChanged: () => void }) {
  const [busy, setBusy] = useState(false)
  const [closed, setClosed] = useState(false)
  const ownerOnly = 'Only the playlist’s owner can do that. Ask them for the invite link.'
  const invite = async () => {
    setBusy(true)
    try {
      const link = await ytmInvite(mine.playlist_id)
      await navigator.clipboard?.writeText(link).catch(() => {})
      notify(
        mine.collaborative
          ? 'Invite link copied.'
          : 'Invite link copied. Anyone signed in to YouTube Music can add songs with it; each person can take out only what they added.',
      )
      if (!mine.collaborative) onChanged()
    } catch (e) {
      notify(mine.owned ? onlineProblem(e) : ownerOnly)
    } finally {
      setBusy(false)
    }
  }
  const close = async (next: boolean) => {
    setBusy(true)
    try {
      await ytmCloseToCollaborators(mine.playlist_id, next)
      setClosed(next)
      notify(next ? 'No one new can join now. Everyone in it can still add songs.' : 'People can join with the invite link again.')
    } catch (e) {
      notify(mine.owned ? onlineProblem(e) : ownerOnly)
    } finally {
      setBusy(false)
    }
  }
  return (
    <div className="mt-4 flex max-w-[640px] flex-wrap items-center gap-3 rounded-2xl bg-ink/[0.04] px-4 py-3">
      {mine.collaborative && mine.people.length > 0 && (
        <span className="flex -space-x-2" aria-hidden>
          {mine.people.slice(0, 5).map((p) => (
            <span key={p} className="h-7 w-7 overflow-hidden rounded-full bg-ink/10 ring-2 ring-[var(--surface,transparent)]">
              <RemoteArt url={p} size={28} imgClassName="h-full w-full object-cover" />
            </span>
          ))}
        </span>
      )}
      <p className="min-w-[180px] flex-1 text-[12.5px] leading-relaxed text-ink/60">
        {mine.collaborative
          ? `${mine.people_text ? `${mine.people_text}. ` : ''}Anyone in it adds songs; each person can take out only what they added.`
          : 'Invite people to add songs to it with you.'}
      </p>
      <button onClick={() => void invite()} disabled={busy} className="btn btn-sm">
        {mine.collaborative ? 'Copy invite link' : 'Invite people'}
      </button>
      {mine.collaborative && (
        <button onClick={() => void close(!closed)} disabled={busy} className="btn-ghost btn-sm">
          {closed ? 'Let people join again' : 'Stop new people joining'}
        </button>
      )}
    </div>
  )
}
