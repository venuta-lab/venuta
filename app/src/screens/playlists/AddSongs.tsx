import { motion } from 'motion/react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { listeningSummary, quality, type CatalogTrack, type ListeningSummary, type OnlineTrack } from '../../lib/engine'
import { useCovers } from '../../lib/covers'
import { useEscape } from '../../lib/layers'
import { songLength } from '../../lib/player'
import { IconClose, IconSearch } from '../../components/Icons'
import { AvailabilityBadge } from '../../components/AvailabilityBadge'
import { RemoteArt } from '../../components/RemoteArt'
import { indexTracks, search, type Album } from '../listen/model'
import { useOnlineSearch } from '../listen/onlineSearch'
import { QualityBadge } from '../listen/parts'
import { onlineId, plural } from './common'

/*
  Add songs to a playlist: one search, the way music apps do it. What you type finds songs in
  your library first (ranked the same as Music's search), then whole albums, and then songs
  online, which arrive a moment later and never hold up your library. Every row has one Add that
  turns into a tick; songs already in the playlist show as added. ↑ and ↓ move through all of it,
  Enter adds, Escape clears what you typed and then closes. Before you type: the songs you play
  most, and songs from the albums you played lately.
*/

const SONGS = 8
const ALBUMS = 4
const ONLINE = 8

type Entry =
  | { kind: 'song'; key: string; t: CatalogTrack }
  | { kind: 'album'; key: string; a: Album }
  | { kind: 'online'; key: string; t: OnlineTrack }

export function AddSongs(props: {
  root: string
  catalog: CatalogTrack[] | null
  /** What the playlist has: library songs by path, online songs by their id. */
  has: { local: Set<string>; online: Set<string> }
  onAddLocal: (tracks: CatalogTrack[]) => void
  onAddOnline: (t: OnlineTrack) => void
  onClose: () => void
}) {
  const [q, setQ] = useState('')
  const [sel, setSel] = useState(0)
  const [more, setMore] = useState(0)
  // Ticks shown at once, before the playlist has been saved and read again.
  const [added, setAdded] = useState<Set<string>>(() => new Set())
  const [said, setSaid] = useState('')
  const input = useRef<HTMLInputElement>(null)
  const list = useRef<HTMLDivElement>(null)
  const cover = useCovers(320)
  useEscape(true, props.onClose)

  const lib = useMemo(() => indexTracks(props.catalog ?? []), [props.catalog])
  const found = useMemo(() => search(lib, q), [lib, q])
  const online = useOnlineSearch(q)
  const onlineFresh = online.resultsFor === q.trim()
  const onlineTracks = useMemo(() => (q.trim() ? (online.results?.tracks ?? []).slice(0, ONLINE) : []), [online.results, q])

  // Before anything is typed: what you play most, and songs from albums you played lately.
  const [heard, setHeard] = useState<ListeningSummary | null>(null)
  useEffect(() => {
    let live = true
    void listeningSummary(props.root)
      .then((h) => live && setHeard(h))
      .catch(() => {})
    return () => {
      live = false
    }
  }, [props.root])
  const suggested = useMemo(() => {
    if (!heard || !props.catalog) return { most: [] as CatalogTrack[], lately: [] as CatalogTrack[] }
    const byPath = new Map(props.catalog.map((t) => [t.path, t]))
    const most = Object.entries(heard.counts)
      .sort((a, b) => b[1] - a[1])
      .map(([p]) => byPath.get(p))
      .filter((t): t is CatalogTrack => !!t)
      .slice(0, SONGS)
    const seen = new Set(most.map((t) => t.path))
    const lately = heard.recent
      .flatMap((d) => lib.byDir.get(d)?.tracks ?? [])
      .filter((t) => !seen.has(t.path))
      .slice(0, SONGS)
    return { most, lately }
  }, [heard, props.catalog, lib])

  // What is typed changes: back to the first result, and the first few songs.
  const type = (text: string) => {
    setQ(text)
    setSel(0)
    setMore(0)
  }

  const typed = !!q.trim()
  const songs = typed ? found.tracks.slice(0, SONGS + more) : []
  const albums = typed ? found.albums.slice(0, ALBUMS) : []
  const entries: Entry[] = typed
    ? [
        ...songs.map((t): Entry => ({ kind: 'song', key: t.path, t })),
        ...albums.map((a): Entry => ({ kind: 'album', key: `album:${a.dir}`, a })),
        ...onlineTracks.map((t): Entry => ({ kind: 'online', key: `online:${onlineId(t)}`, t })),
      ]
    : [...suggested.most, ...suggested.lately].map((t): Entry => ({ kind: 'song', key: t.path, t }))

  const isIn = (e: Entry) =>
    e.kind === 'song'
      ? props.has.local.has(e.t.path) || added.has(e.key)
      : e.kind === 'online'
        ? props.has.online.has(onlineId(e.t)) || added.has(e.key)
        : e.a.tracks.every((t) => props.has.local.has(t.path) || added.has(t.path)) || added.has(e.key)

  const add = (e: Entry) => {
    if (isIn(e)) return
    const next = new Set(added)
    if (e.kind === 'song') {
      props.onAddLocal([e.t])
      setSaid(`Added ${e.t.title}.`)
    } else if (e.kind === 'online') {
      props.onAddOnline(e.t)
      setSaid(`Added ${e.t.title}, online.`)
    } else {
      const fresh = e.a.tracks.filter((t) => !props.has.local.has(t.path) && !added.has(t.path))
      props.onAddLocal(fresh)
      for (const t of fresh) next.add(t.path)
      setSaid(`Added ${fresh.length === 1 ? fresh[0].title : `${fresh.length} songs from ${e.a.title}`}.`)
    }
    next.add(e.key)
    setAdded(next)
  }

  // The chosen row stays in sight while ↑ and ↓ move through the sections.
  useEffect(() => {
    list.current?.querySelector(`[data-i="${sel}"]`)?.scrollIntoView({ block: 'nearest' })
  }, [sel])

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      if (q) type('')
      else props.onClose()
    } else if (e.key === 'ArrowDown') {
      e.preventDefault()
      setSel((i) => Math.min(i + 1, Math.max(0, entries.length - 1)))
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setSel((i) => Math.max(i - 1, 0))
    } else if (e.key === 'Enter') {
      e.preventDefault()
      const pick = entries[sel]
      if (pick) add(pick)
    }
  }

  let i = 0
  const row = (e: Entry) => {
    const n = i++
    return <Row key={e.key} entry={e} n={n} on={n === sel} inIt={isIn(e)} cover={cover} onHover={() => setSel(n)} onAdd={() => add(e)} />
  }
  const heading = (text: string, aside?: React.ReactNode) => (
    <div className="flex items-baseline gap-2 px-2 pb-1 pt-3 text-[12px] font-medium text-ink/50">
      <span>{text}</span>
      {aside}
    </div>
  )
  const note = (text: string) => <p className="px-2 py-2 text-[12.5px] text-ink/50">{text}</p>

  return (
    <motion.section
      initial={{ opacity: 0, y: -6 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -6 }}
      className="glass mt-8 rounded-[20px] p-4"
      aria-label="Add songs"
    >
      <div className="flex items-center gap-3 rounded-full bg-ink/[0.07] px-4 shadow-[inset_0_0_0_1px_var(--edge)] focus-within:bg-ink/[0.1] focus-within:shadow-[inset_0_0_0_1px_color-mix(in_oklab,var(--accent)_60%,transparent)]">
        <IconSearch className="shrink-0 text-ink/45" />
        <input
          ref={input}
          autoFocus
          value={q}
          onChange={(e) => type(e.target.value)}
          onKeyDown={onKey}
          spellCheck={false}
          autoCorrect="off"
          autoComplete="off"
          data-escape="layer"
          placeholder="Songs, artists or albums, in your library and online"
          aria-label="Find songs to add"
          aria-controls="add-songs-results"
          className="min-w-0 flex-1 bg-transparent py-2.5 text-[14px] outline-none placeholder:text-ink/45"
        />
        {typed && online.loading && (
          <span className="spin h-3.5 w-3.5 shrink-0 rounded-full border-2 border-ink/20 border-t-ink/60" aria-label="Searching online" />
        )}
        {q && (
          <button
            aria-label="Clear"
            onClick={() => {
              type('')
              input.current?.focus()
            }}
            className="grid h-6 w-6 shrink-0 place-items-center rounded-full text-ink/50 hover:bg-ink/10 hover:text-ink"
          >
            <IconClose width={14} height={14} />
          </button>
        )}
      </div>
      <p aria-live="polite" className="sr-only">
        {said}
      </p>

      <div ref={list} id="add-songs-results" role="listbox" aria-label="Songs to add" className="fade-y mt-1 grid max-h-[420px] gap-0.5 overflow-y-auto pb-1">
        {!typed && !props.catalog && note('Reading your library…')}
        {!typed && props.catalog && !entries.length && note('Type a song, an artist or an album. Your library comes first, then songs online.')}
        {!typed && suggested.most.length > 0 && heading('You play these most')}
        {!typed && suggested.most.map((t) => row({ kind: 'song', key: t.path, t }))}
        {!typed && suggested.lately.length > 0 && heading('From albums you played lately')}
        {!typed && suggested.lately.map((t) => row({ kind: 'song', key: t.path, t }))}

        {typed && heading('In your library', found.tracks.length > 0 && <span className="font-normal">{found.tracks.length.toLocaleString('en-IN')} {found.tracks.length === 1 ? 'song' : 'songs'}</span>)}
        {typed && !props.catalog && note('Reading your library…')}
        {typed && props.catalog && !songs.length && !albums.length && note(`Nothing in your library matches “${q.trim()}”.`)}
        {songs.map((t) => row({ kind: 'song', key: t.path, t }))}
        {typed && found.tracks.length > songs.length && (
          <button onClick={() => setMore((m) => m + 20)} className="mx-2 my-1 self-start rounded-full px-3 py-1 text-[12.5px] text-ink/65 hover:bg-ink/[0.08] hover:text-ink">
            Show {Math.min(20, found.tracks.length - songs.length)} more
          </button>
        )}
        {albums.length > 0 && heading('Albums')}
        {albums.map((a) => row({ kind: 'album', key: `album:${a.dir}`, a }))}

        {typed && heading('Online', <span className="font-normal">plays online until you have the file</span>)}
        {typed && online.loading && !onlineTracks.length && <Skeleton />}
        {typed && !online.loading && onlineFresh && online.error && note(online.error)}
        {typed && !online.loading && onlineFresh && !online.error && !onlineTracks.length && note(`Nothing online matches “${q.trim()}”.`)}
        {onlineTracks.map((t) => row({ kind: 'online', key: `online:${onlineId(t)}`, t }))}
      </div>
    </motion.section>
  )
}

function Row(props: {
  entry: Entry
  n: number
  on: boolean
  inIt: boolean
  cover: ReturnType<typeof useCovers>
  onHover: () => void
  onAdd: () => void
}) {
  const e = props.entry
  const art =
    e.kind === 'online' ? null : e.kind === 'song' ? props.cover(e.t.dir) : props.cover(e.a.dir)
  const title = e.kind === 'album' ? e.a.title : e.t.title
  const line =
    e.kind === 'song'
      ? `${e.t.artists.join(', ') || e.t.album_artist} · ${e.t.album}`
      : e.kind === 'album'
        ? `${e.a.artist}${e.a.year ? `, ${e.a.year}` : ''} · ${plural(e.a.tracks.length, 'song')}`
        : `${e.t.artists.join(', ') || 'Unknown artist'}${e.t.album ? ` · ${e.t.album}` : ''}`
  const what = e.kind === 'album' ? `all of ${title}` : title
  return (
    <div
      role="option"
      aria-selected={props.on}
      aria-label={`${title}, ${line.replace(' · ', ', ')}${e.kind === 'online' ? ', online' : ''}${props.inIt ? ', in the playlist' : ''}`}
      data-i={props.n}
      onMouseMove={props.onHover}
      onClick={props.onAdd}
      className={`group flex cursor-pointer items-center gap-3 rounded-xl px-2 py-1.5 transition-colors ${props.on ? 'bg-ink/[0.07]' : 'hover:bg-ink/[0.05]'}`}
    >
      <span aria-hidden className={`grid h-10 w-10 shrink-0 place-items-center overflow-hidden bg-ink/[0.07] text-ink/30 ${e.kind === 'album' ? 'rounded-lg' : 'rounded-md'}`}>
        {e.kind === 'online' ? (
          <RemoteArt url={e.t.artwork_url} seed={e.t.album || e.t.title} size={40} imgClassName="h-full w-full object-cover" />
        ) : art ? (
          <img src={art.src} alt="" className="h-full w-full object-cover" draggable={false} />
        ) : (
          '♪'
        )}
      </span>
      <span className="min-w-0 flex-1">
        <span className="flex min-w-0 items-center gap-1.5">
          <span className="truncate text-[13.5px]">{title}</span>
          {e.kind === 'online' && <AvailabilityBadge availability="online" />}
        </span>
        <span className="block truncate text-[12px] text-ink/55">{line}</span>
      </span>
      {e.kind === 'song' && (
        <span className="hidden shrink-0 sm:block">
          <QualityBadge text={quality(e.t)} />
        </span>
      )}
      {e.kind !== 'album' && <span className="w-10 shrink-0 text-right text-[12px] tabular-nums text-ink/45">{songLength(e.t.duration_ms)}</span>}
      <button
        tabIndex={-1}
        disabled={props.inIt}
        aria-label={props.inIt ? `${title} is in the playlist` : `Add ${what}`}
        title={props.inIt ? 'In the playlist' : e.kind === 'album' ? 'Add all its songs' : 'Add'}
        onClick={(ev) => {
          ev.stopPropagation()
          props.onAdd()
        }}
        className={`grid h-8 shrink-0 place-items-center rounded-full px-2.5 text-[12.5px] font-medium transition-colors ${
          props.inIt ? 'text-accent-ink' : 'text-ink/70 hover:bg-ink/10 hover:text-ink'
        }`}
      >
        {props.inIt ? (
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
            <path d="m5 12 5 5 9-10" />
          </svg>
        ) : e.kind === 'album' ? (
          'Add all'
        ) : (
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" aria-hidden>
            <path d="M12 5v14M5 12h14" />
          </svg>
        )}
      </button>
    </div>
  )
}

function Skeleton() {
  return (
    <div className="grid gap-1.5 px-2 py-2" aria-busy="true">
      <p className="sr-only">Searching online…</p>
      {Array.from({ length: 3 }, (_, i) => (
        <div key={i} className="flex items-center gap-3" style={{ opacity: 1 - i * 0.15 }}>
          <span className="shimmer h-10 w-10 rounded-md" />
          <span className="grid min-w-0 flex-1 gap-1.5">
            <span className="shimmer h-3 w-2/3 rounded-full" />
            <span className="shimmer h-2.5 w-1/2 rounded-full" />
          </span>
        </div>
      ))}
    </div>
  )
}
