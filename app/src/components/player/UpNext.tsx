import { useEffect, useMemo, useRef, useState } from 'react'
import { askAddSongs } from '../AddToPlaylist'
import { AvailabilityBadge } from '../AvailabilityBadge'
import { DragHandle, MOVE_KEYS } from '../DragHandle'
import { IconClose, IconMore } from '../Icons'
import { RemoteArt } from '../RemoteArt'
import { Chip } from '../ui'
import { CARD_FADE, CHIP_FADE, ScrollRow } from '../ScrollRow'
import { WindowedList, type Grab, type Line, type WindowedListHandle } from '../WindowedList'
import { PlayingFrom } from './PlayingFrom'
import { goToEach, menuFor, ytmMenuWork } from '../../lib/actions'
import { useCovers } from '../../lib/covers'
import type { OnlineSuggestion, OnlineTrack } from '../../lib/engine'
import { useLocalOnly } from '../../lib/localOnly'
import { menuFromEvent } from '../../lib/menu'
import { onlineProblem } from '../../lib/online'
import { knownLength } from '../../lib/onlineSong'
import {
  availabilityOf,
  current,
  enqueueItems,
  isOnline,
  jumpTo,
  move,
  onlineEntryFrom,
  openFrom,
  queueKey,
  readFrom,
  removeAt,
  songItemOf,
  songLength,
  toRemoteItem,
  toggle,
  usePlayer,
  usePlayerValue,
  type From,
  type QueueItem,
} from '../../lib/player'
import { chooseChip, playWithRadio, relatedPage, startRadio, useRadioSession } from '../../lib/radio'
import { canPlayNext, chipsShown, onlineAlbumOf, placeOf, queueName, queueNote, relatedParts, rowLabel, saveName, type RelatedPart } from '../../lib/upNext'
import { openMusicPage } from '../../lib/musicPage'
import { CardWidth, NavContext, type Nav } from '../../screens/listen/model'
import { useOnlineCards } from '../../screens/listen/useOnlineCards'

/*
  Up next, as YouTube Music's player shows it: where the queue plays from, its tuning chips, and
  every song in it as YouTube sent it (played ones dimmed above, the one playing marked, the ones
  to come below, each saying LOCAL or ONLINE when the queue holds online songs). Tap a song to
  play it (the queue stays as it is), drag or Alt+Up/Down to move one still to come, × or Delete
  to take one out, right-click or … for the rest. Related is YouTube's songs like the one playing.
  The queue drawer and Now Playing both show these; lib/upNext.ts has the rules.
*/

/** One row's height; the "Up next" heading's. */
const ROW_H = 50
const HEAD_H = 34

const ROW = 'group absolute inset-x-0 flex min-w-0 cursor-pointer items-center gap-3 rounded-xl px-2'

/** "Playing from Kitni Baatein Radio": YouTube Music's own name for the radio, else where the
 *  queue came from (a link back to it). */
export function QueueFrom(props: { className?: string; onOpen?: () => void; said?: string[] }) {
  const name = queueName(useRadioSession())
  if (!name) return <PlayingFrom className={props.className} onOpen={props.onOpen} said={props.said} />
  return (
    <span className={props.className}>
      Playing from <span className="text-ink/80">{name}</span>
    </span>
  )
}

/** The radio's tuning chips, in YouTube's order; choosing one re-tunes what is coming up. None in
 *  a room, and none when YouTube sent none. */
export function QueueChips({ className = '' }: { className?: string }) {
  const view = useRadioSession()
  const chips = chipsShown(view)
  if (!chips.length) return null
  return (
    <ScrollRow role="group" aria-label="Tune the radio" aria-busy={view?.loading || undefined} fade={CHIP_FADE} className={`gap-1 ${className}`}>
      {chips.map((c, i) => (
        <Chip key={`${c.text}:${i}`} on={c.selected} label={c.text} group="queue-chip" onClick={() => !view?.loading && void chooseChip(i)} />
      ))}
    </ScrollRow>
  )
}

/** Under the queue: a word while more is coming, the end in words when nothing more will. */
export function QueueNote({ className = '' }: { className?: string }) {
  const note = queueNote(useRadioSession())
  return (
    <p role="status" aria-live="polite" className={`text-center text-[12px] text-ink/50 ${className}`}>
      {note && (
        <span className="inline-flex items-center gap-1.5">
          {note.endsWith('…') && <span className="spin h-2.5 w-2.5 rounded-full border-2 border-ink/20 border-t-ink/55" aria-hidden />}
          {note}
        </span>
      )}
    </p>
  )
}

/** Every song in the queue saved as a playlist, its name the radio's until another is typed. */
function saveQueue(queue: QueueItem[], view: ReturnType<typeof useRadioSession>, from: string) {
  askAddSongs(queue.map(songItemOf), 'the queue', saveName(view, readFrom(from)?.label ?? ''))
}

/** The Save as playlist button, as the queue drawer has it. */
export function SaveQueueButton({ className = '' }: { className?: string }) {
  const queue = usePlayerValue((s) => s.queue)
  const from = usePlayerValue((s) => s.from)
  const view = useRadioSession()
  if (!queue.length) return null
  return (
    <button onClick={() => saveQueue(queue, view, from)} className={`btn-ghost btn-sm shrink-0 ${className}`} title="Save every song in the queue as a playlist, or add them to one">
      Save as playlist
    </button>
  )
}

/** A song in the queue as the list shows it: the song and its place in the queue. */
type Row = { q: QueueItem; at: number }

/** One song: cover, title, artist, where it plays from, length, and its × (its menu is a
 *  right-click, or Shift+F10 and the menu key from the keyboard). */
function RowBody(props: { item: QueueItem; current: boolean; badge: boolean; onRemove?: () => void }) {
  const cover = useCovers(320)
  const online = isOnline(props.item)
  const local = !online ? cover(props.item.dir) : null
  // A song YouTube queued without its length (often the one it opens with): while it plays, the
  // player knows it.
  const playingLength = usePlayerValue((s) => s.duration_ms)
  const length = props.item.duration_ms || (props.current ? playingLength : 0)
  return (
    <>
      <div className="grid h-9 w-9 shrink-0 place-items-center overflow-hidden rounded-lg bg-ink/[0.08] text-ink/30">
        {online ? (
          <RemoteArt url={props.item.artwork_url} seed={props.item.album || props.item.title} size={36} imgClassName="h-full w-full object-cover" />
        ) : local ? (
          <img src={local.src} alt="" className="h-full w-full object-cover" draggable={false} />
        ) : null}
      </div>
      <div className="min-w-0 flex-1">
        <div className={`flex min-w-0 items-center gap-1.5 text-[13px] ${props.current ? 'font-medium text-accent-ink' : ''}`}>
          <span className="truncate">{props.item.title}</span>
        </div>
        <div className="truncate text-[11.5px] text-ink/55">{props.item.artists.join(', ') || props.item.album_artist}</div>
      </div>
      {props.badge && <AvailabilityBadge quiet availability={availabilityOf(props.item)} />}
      <span className="text-[11.5px] tabular-nums text-ink/45">{songLength(length)}</span>
      {props.onRemove ? (
        <button
          aria-label={`Remove ${props.item.title} from the queue`}
          onClick={props.onRemove}
          className="grid h-6 w-6 place-items-center rounded-full text-ink/45 opacity-0 hover:bg-ink/10 hover:text-ink focus:opacity-100 group-hover:opacity-100"
        >
          <IconClose width={14} height={14} />
        </button>
      ) : (
        <span aria-hidden className="w-6 shrink-0" />
      )}
    </>
  )
}

/**
 * Every song in the queue, drawn a screen at a time (a queue of the whole library opens as fast
 * as an album) inside whatever scrolls around it. `open`: brings the song playing into view each
 * time it turns true. `onLeave`: going to an album or artist leaves where this is shown.
 */
export function QueueList(props: { open: boolean; onLeave?: () => void }) {
  const s = usePlayer()
  const list = useRef<WindowedListHandle>(null)
  const root = s.root ?? ''

  // The player keeps the same queue until it changes, so this is rebuilt only then (not on
  // every position update).
  const { lines, firstNext } = useMemo(() => {
    const out: Line<Row>[] = []
    s.queue.forEach((q, at) => {
      if (at === s.index + 1) out.push({ kind: 'head', key: 'up-next', node: <div className="px-2 text-[12px] text-ink/50">Up next</div> })
      out.push({ kind: 'row', key: `${queueKey(q)}#${at}`, row: { q, at } })
    })
    return { lines: out, firstNext: s.index + 2 }
  }, [s.queue, s.index])

  // Bring the playing song into view when shown, with the one before it in sight.
  const { open } = props
  useEffect(() => {
    if (!open) return
    const t = window.setTimeout(() => list.current?.scrollToLine(Math.max(0, s.index), ROW_H + 8), 30)
    return () => window.clearTimeout(t)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open])

  // Where each song plays from is said only when the queue holds online songs.
  const mixed = useMemo(() => s.queue.some(isOnline), [s.queue])

  const go = (f: From) => {
    openFrom(f)
    props.onLeave?.()
  }
  // Right-click or …: what lib/actionModel.ts says a queued song can do.
  const rowMenu = (r: Row) => {
    const q = r.q
    const isCurrent = r.at === s.index
    const online = isOnline(q)
    const names = q.artists.length ? q.artists : q.album_artist ? [q.album_artist] : []
    // An online song goes to its online album, when YouTube named one with it.
    const onlineAlbum = online ? onlineAlbumOf(q) : null
    return menuFor('queued', { current: isCurrent, online, noArtist: !names.length, onlineAlbum: !!onlineAlbum }, {
      play: isCurrent ? undefined : () => void jumpTo(r.at),
      next: canPlayNext(r.at, s.index) ? () => void enqueueItems(root, [q], true) : undefined,
      radio: () => void startRadio(root, q),
      playlist: () => askAddSongs([songItemOf(q)], q.title),
      artist: goToEach(names, (n) => go({ kind: 'artist', ref: n, label: n })),
      album: onlineAlbum
        ? () => openMusicPage({ kind: 'online-album', album: onlineAlbum })
        : online || !q.dir
          ? undefined
          : () => go({ kind: 'album', ref: q.dir, label: q.album }),
      remove: isCurrent ? undefined : () => void removeAt(r.at),
      // An online song's own YouTube Music menu, as it was listed (save, pin, not interested).
      ...(online ? ytmMenuWork(queueKey(q), q.title) : {}),
    })
  }

  if (!s.queue.length) return <p className="px-2 py-10 text-center text-[13px] text-ink/55">Play an album or a song, and it lines up here.</p>

  const row = (r: Row, _line: number, style: React.CSSProperties, grab: Grab | null) => {
    const place = placeOf(r.at, s.index)
    const isCurrent = place === 'current'
    const where = mixed ? (availabilityOf(r.q) === 'online' ? 'Online' : availabilityOf(r.q) === 'room' ? 'From the host' : 'Local') : null
    return (
      <div
        role="listitem"
        aria-label={rowLabel({ title: r.q.title, artists: r.q.artists, length: r.q.duration_ms > 0 ? songLength(r.q.duration_ms) : '', where, place })}
        aria-current={isCurrent || undefined}
        aria-keyshortcuts={grab ? `${MOVE_KEYS} Delete Shift+F10` : isCurrent ? 'Shift+F10' : 'Delete Shift+F10'}
        tabIndex={0}
        style={style}
        data-current={isCurrent}
        onPointerDown={grab?.onGrab}
        onContextMenu={(e) => menuFromEvent(e, rowMenu(r))}
        onClick={(e) => {
          if ((e.target as HTMLElement).closest('button') || grab?.justDropped()) return
          void (isCurrent ? toggle() : jumpTo(r.at))
        }}
        onKeyDown={(e) => {
          if (e.target !== e.currentTarget) return
          if (e.key === 'Enter') void (isCurrent ? toggle() : jumpTo(r.at))
          // Alt+arrows move a song without dragging.
          if (grab && e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
            e.preventDefault()
            grab.nudge(e.key === 'ArrowUp' ? -1 : 1)
          }
          if (e.key === 'Delete' && !isCurrent) void removeAt(r.at)
        }}
        title={grab ? 'Drag to move; Alt+↑/↓ with the keyboard' : undefined}
        className={`${ROW} ${
          isCurrent
            ? 'bg-ink/[0.08]'
            : grab?.dragging
              ? 'scale-[1.02] bg-[var(--popover)] shadow-[0_12px_28px_-12px_rgb(0_0_0/0.55)] active:cursor-grabbing'
              : `hover:bg-ink/[0.05] active:bg-ink/[0.09] ${place === 'played' ? 'opacity-70' : ''}`
        }`}
      >
        {/* What comes next can move: the grip says so (the whole row drags too). */}
        {grab ? <DragHandle label={r.q.title} dragging={grab.dragging} /> : <span aria-hidden className="w-3 shrink-0" />}
        <RowBody
          item={r.q}
          current={isCurrent}
          badge={mixed}
          onRemove={isCurrent ? undefined : () => void removeAt(r.at)}
        />
      </div>
    )
  }

  return (
    <WindowedList
      ref={list}
      label="Songs in the queue"
      lines={lines}
      rowHeight={ROW_H}
      headHeight={HEAD_H}
      // Only what comes next can move, and only among what comes next.
      movable={(i) => i >= firstNext}
      bounds={[firstNext, lines.length - 1]}
      onMove={(from, to) => {
        const a = lines[from]
        const b = lines[to]
        if (a?.kind === 'row' && b?.kind === 'row') void move(a.row.at, b.row.at)
      }}
      render={row}
    />
  )
}

/** Related pages already asked for, by the song they are for (asked once a window). */
const relatedKept = new Map<string, RelatedPart[]>()

/** How wide a Related card is: a row of them fits the drawer. */
const CARD_W = 132

/** Pages of Music opened from here: Music shows them, and the drawer or Now Playing closes. */
function useOutsideNav(root: string): Nav {
  return useMemo(
    () => ({
      root,
      openAlbum: (dir) => openFrom({ kind: 'album', ref: dir, label: '' }),
      openArtist: (name) => openFrom({ kind: 'artist', ref: name, label: name }),
      openArtistNamed: (name) => openFrom({ kind: 'artist', ref: name, label: name }),
      openOnlineAlbum: (album) => openMusicPage({ kind: 'online-album', album }),
      openOnlineArtist: (artist) => openMusicPage({ kind: 'online-artist', artist }),
      openOnlineCollection: (collection) => openMusicPage({ kind: 'online-collection', collection }),
      openMood: (tile) => openMusicPage({ kind: 'mood', tile }),
      openOnlineShelf: (title, owner, more) => openMusicPage({ kind: 'online-shelf', title, owner, more }),
      openMixes: (title) => openMusicPage({ kind: 'mixes', title }),
      editDetails: () => {},
    }),
    [root],
  )
}

const shelfHead = 'px-2 pb-1.5 text-[12px] text-ink/50'

/** A row of Related cards (artists, albums, playlists), the same cards as everywhere online:
 *  an album or playlist plays as its own play button plays it, an artist opens their page. */
function RelatedCards({ title, items, label }: { title: string; items: OnlineSuggestion[]; label: string }) {
  const cards = useOnlineCards(items, label, CARD_W)
  return (
    <section aria-label={title}>
      {title && <h3 className={shelfHead}>{title}</h3>}
      <ScrollRow arrows={{ top: Math.round(CARD_W * 0.38), inside: true }} fade={CARD_FADE} className="snap-x gap-3 px-2 pb-1" style={{ scrollPaddingInline: 8 }}>
        {cards.map((c) => (
          <div key={c.key} className="shrink-0 snap-start" style={{ width: CARD_W }}>
            {c.node}
          </div>
        ))}
      </ScrollRow>
    </section>
  )
}

/** YouTube Music's Related tab for the song playing, shelf by shelf as YouTube shows it: its
 *  songs (each plays with its radio, or goes next or at the end of the queue), then rows of the
 *  artists, albums and playlists it relates, and what YouTube says about the artist. Nothing is
 *  asked in Local only. */
export function RelatedList() {
  const song = usePlayerValue(current)
  const root = usePlayerValue((s) => s.root ?? '')
  const localOnly = useLocalOnly()
  const nav = useOutsideNav(root)
  const key = song ? queueKey(song) : ''
  const [, tick] = useState(0)
  const [problem, setProblem] = useState<{ key: string; words: string } | null>(null)
  const parts = relatedKept.get(key)
  useEffect(() => {
    if (!song || localOnly || relatedKept.has(key)) return
    let live = true
    relatedPage(song)
      .then((found) => {
        relatedKept.set(key, relatedParts(found))
        if (live) tick((n) => n + 1)
      })
      .catch((e) => live && setProblem({ key, words: onlineProblem(e) }))
    return () => {
      live = false
    }
    // The song by its key: a new report of the same song asks nothing.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, localOnly])

  const say = (words: string) => <p className="px-2 py-10 text-center text-[13px] text-ink/55">{words}</p>
  if (!song) return say('Play a song, and the music YouTube Music relates to it shows here.')
  if (localOnly) return say("Local only is on, so there's nothing from YouTube Music here.")
  if (problem?.key === key && !parts) return say(problem.words)
  if (!parts) return say('Finding related music…')
  if (!parts.length) return say('YouTube Music has nothing related to this one.')
  return (
    <NavContext value={nav}>
      <CardWidth value={CARD_W}>
        <div aria-label={`Related to ${song.title}`} className="grid gap-5">
          {parts.map((p, i) =>
            p.kind === 'songs' ? (
              <section key={`${p.title}:${i}`} aria-label={p.title}>
                {p.title && <h3 className={shelfHead}>{p.title}</h3>}
                <RelatedSongs tracks={p.tracks} root={root} label={p.title || `Related to ${song.title}`} />
              </section>
            ) : p.kind === 'cards' ? (
              <RelatedCards key={`${p.title}:${i}`} title={p.title} items={p.items} label={song.title} />
            ) : (
              <section key={`${p.title}:${i}`} aria-label={p.title}>
                {p.title && <h3 className={shelfHead}>{p.title}</h3>}
                <p className="line-clamp-6 px-2 text-[12.5px] leading-relaxed text-ink/70">{p.about}</p>
              </section>
            ),
          )}
        </div>
      </CardWidth>
    </NavContext>
  )
}

/** Related's songs as a list. A length YouTube didn't send shows as nothing. */
function RelatedSongs({ tracks, root, label }: { tracks: OnlineTrack[]; root: string; label: string }) {
  return (
    <ul aria-label={label} className="grid gap-0.5">
      {tracks.map((t, i) => {
        const item = toRemoteItem(t)
        const menu = menuFor(
          'song',
          { online: true, noArtist: true },
          {
            play: () => void playWithRadio(root, item),
            next: () => void enqueueItems(root, [item], true),
            queue: () => void enqueueItems(root, [item], false),
            radio: () => void startRadio(root, item),
            playlist: () => askAddSongs([{ online: onlineEntryFrom(t) }], t.title),
            ...ytmMenuWork(queueKey(item), t.title, t.menu),
          },
        )
        return (
          <li key={`${t.track_id}#${i}`} className="group flex min-w-0 items-center gap-1 rounded-xl hover:bg-ink/[0.05]" onContextMenu={(e) => menuFromEvent(e, menu)}>
            <button onClick={() => void playWithRadio(root, item)} title={`Play “${t.title}” and songs like it`} className="flex min-w-0 flex-1 items-center gap-3 rounded-xl px-2 py-1.5 text-left">
              <span className="grid h-9 w-9 shrink-0 place-items-center overflow-hidden rounded-lg bg-ink/[0.08] text-ink/30">
                <RemoteArt url={item.artwork_url} seed={t.album || t.title} size={36} imgClassName="h-full w-full object-cover" />
              </span>
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[13px]">{t.title}</span>
                <span className="block truncate text-[11.5px] text-ink/55">{t.artists.join(', ')}</span>
              </span>
              <AvailabilityBadge quiet availability="online" />
              <span className="text-[11.5px] tabular-nums text-ink/45">{knownLength(t.duration_ms)}</span>
            </button>
            <button
              aria-label={`More for ${t.title}: Play next, Add to queue`}
              aria-haspopup="menu"
              onClick={(e) => menuFromEvent(e, menu)}
              className="mr-1 grid h-7 w-7 shrink-0 place-items-center rounded-full text-ink/45 opacity-0 hover:bg-ink/10 hover:text-ink focus:opacity-100 group-hover:opacity-100"
            >
              <IconMore width={14} height={14} />
            </button>
          </li>
        )
      })}
    </ul>
  )
}
