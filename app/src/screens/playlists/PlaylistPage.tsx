import { AnimatePresence, motion } from 'motion/react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { save as saveDialog } from '@tauri-apps/plugin-dialog'
import { tracksFor } from '../../lib/catalog'
import { useCovers } from '../../lib/covers'
import {
  confirmBackup,
  playlistDelete,
  playlistDuplicate,
  playlistExport,
  playlistQueue,
  playlistRefind,
  playlistRename,
  playlistSaveItems,
  playlistTracks,
  ratePlaylist,
  searchWeb,
  smartSave,
  type CatalogTrack,
  type OnlineEntry,
  type OnlineTrack,
  type PlaylistEntry,
  type PlaylistItem,
  type PlaylistSummary,
  type SmartRule,
} from '../../lib/engine'
import {
  clock,
  current,
  enqueue,
  enqueueItems,
  followItems,
  fromIs,
  onlineEntryFrom,
  onlineItem,
  playAllItems,
  playItems,
  queueKey,
  toggle,
  usePlayerValue,
  type QueueItem,
} from '../../lib/player'
import { notify } from '../../lib/notify'
import { keyFor, matcher } from '../../lib/textsearch'
import { useEscape } from '../../lib/layers'
import { confirmAsk } from '../../components/Confirm'
import { askAddSongs, askAddToPlaylist } from '../../components/AddToPlaylist'
import { AvailabilityBadge } from '../../components/AvailabilityBadge'
import { Mosaic } from '../../components/Mosaic'
import { RemoteArt } from '../../components/RemoteArt'
import { Detail, Segmented } from '../../components/ui'
import { IconMore, IconPause, IconPlay, IconSearch, IconShuffle } from '../../components/Icons'
import { useOnlineSearch } from '../listen/OnlineResults'
import { Equalizer, useContextMenu } from '../listen/parts'
import { entryAvailability, fromWhere, length, playlistFrom, plural, queueOf, sourceDate } from './common'
import { OnlinePick } from './OnlinePick'
import { describeRule, SmartRules } from './SmartRules'
import { WindowedList, type Grab, type Line } from '../../components/WindowedList'

type Row = PlaylistEntry & { key: string }
let keySeq = 0
const withKeys = (entries: PlaylistEntry[]): Row[] => entries.map((e) => ({ ...e, key: `r${++keySeq}` }))
const itemOf = (r: PlaylistEntry): PlaylistItem =>
  r.online
    ? { online: r.online }
    : r.missing
      ? { want: r.want ?? { title: r.title, artist: r.artist, album: r.album, duration_ms: r.duration_ms } }
      : { path: r.path }

/** An online song not in your library (it plays online). */
const isRemote = (r: PlaylistEntry) => r.missing && !!r.online
/** Still to get: not in your library, and not online either. */
const toGet = (r: PlaylistEntry) => r.missing && !r.online
/** The row as the queue tells songs apart (see `queueKey`). */
const rowKey = (r: PlaylistEntry) => (isRemote(r) && r.online ? `online:${r.online.provider}:${r.online.track_id}` : r.path)
/** A row as a line for another playlist: the online song by its id, else the file. */
const songOf = (r: PlaylistEntry): PlaylistItem => (r.online ? { online: r.online } : { path: r.path })

/** Which songs the page shows: all, local files, online-only, or still to get. */
type Show = 'all' | 'here' | 'online' | 'get'
// Remembered per playlist while the app runs.
const shownFor = new Map<string, Show>()

/** The words to look a missing song up by. */
const wordsFor = (r: PlaylistEntry) => [r.artist, r.title].filter(Boolean).join(' ')

/** Missing songs by album, the albums with most songs first: what to get, in the fewest steps. */
function byAlbum(rows: Row[]): { album: string; artist: string; rows: Row[] }[] {
  const groups = new Map<string, { album: string; artist: string; rows: Row[] }>()
  for (const r of rows) {
    const album = r.album.trim()
    const k = album ? album.toLowerCase() : `\u0000${r.key}`
    const g = groups.get(k) ?? { album, artist: r.artist, rows: [] }
    g.rows.push(r)
    groups.set(k, g)
  }
  return [...groups.values()].sort((a, b) => b.rows.length - a.rows.length || a.album.localeCompare(b.album))
}

export function PlaylistPage(props: {
  root: string
  summary: PlaylistSummary | undefined
  name: string
  /** Every song in the library (null while it loads): for playing, adding and finding. */
  catalog: CatalogTrack[] | null
  rule?: SmartRule
  /** A playlist just made: its name ready to type, then the song finder. */
  fresh?: boolean
  /** Open with the name ready to type (Rename, from a card). */
  rename?: boolean
  onBack: () => void
  onChanged: () => void
  onRenamed: (name: string) => void
  onGone: () => void
  onOpen: (name: string) => void
  onAlbum: (dir: string) => void
  /** Bring it up to date from where it came from (reviewed like an import). */
  onUpdate?: (name: string) => void
}) {
  const { root, name } = props
  const smart = !!props.rule
  const [rows, setRows] = useState<Row[] | null>(null)
  const [problem, setProblem] = useState('')
  const [rules, setRules] = useState(false)
  const [adding, setAdding] = useState(false)
  const [finding, setFinding] = useState<Row | null>(null)
  // Choosing online songs for songs to get: one (its menu) or every one (Fill missing online…).
  const [picking, setPicking] = useState<{ rows: Row[]; mode: 'use' | 'play' } | null>(null)
  // Bumped by the menu's Rename: the title turns into a field.
  const [renameAsk, setRenameAsk] = useState(0)
  const latest = useRef<Row[]>([])
  latest.current = rows ?? []
  /** The order as last read or saved, so a drag that ends where it began writes nothing. */
  const saved = useRef('')
  const orderOf = (list: Row[]) => list.map((r) => r.key).join(',')

  const load = useCallback(async () => {
    try {
      let entries: PlaylistEntry[]
      try {
        entries = await playlistTracks(root, name)
      } catch (e) {
        // A smart playlist whose file isn't written yet: writing it is what opening it means.
        // Any other failure (a locked file, a bad line) is shown, never written over.
        const missing = /os error [23]\b|cannot find|not found/i.test(String(e))
        if (!props.rule || !missing) throw e
        await smartSave(root, props.rule)
        entries = await playlistTracks(root, name)
      }
      const keyed = withKeys(entries)
      saved.current = orderOf(keyed)
      setRows(keyed)
      setProblem('')
    } catch (e) {
      setProblem(e instanceof Error ? e.message : String(e))
      setRows([])
    }
  }, [root, name, props.rule])

  useEffect(() => {
    void load()
    const on = () => void load()
    window.addEventListener('venuta:playlists-changed', on)
    return () => window.removeEventListener('venuta:playlists-changed', on)
  }, [load])

  /** Save as shown now; `undo` is offered to put back what was there before. */
  const persist = useCallback(
    async (next: Row[], message?: string, before?: Row[]) => {
      setRows(next)
      saved.current = orderOf(next)
      try {
        await playlistSaveItems(root, name, next.map(itemOf))
        props.onChanged()
        if (message)
          notify(
            message,
            before
              ? {
                  label: 'Undo',
                  run: () => {
                    void playlistSaveItems(root, name, before.map(itemOf)).then(() => {
                      saved.current = orderOf(before)
                      setRows(before)
                      props.onChanged()
                    })
                  },
                }
              : undefined,
          )
      } catch (e) {
        notify(`Couldn't save ${name}: ${e instanceof Error ? e.message : String(e)}`)
        void load()
      }
    },
    [root, name, props, load],
  )

  const have = useMemo(() => (rows ?? []).filter((r) => !r.missing), [rows])
  const online = useMemo(() => (rows ?? []).filter(isRemote), [rows])
  const missing = (rows ?? []).filter(toGet).length
  // Where each song plays from is said only when the playlist holds online songs.
  const mixed = (rows ?? []).some((r) => r.online)
  const [show, setShowState] = useState<Show>(() => shownFor.get(name) ?? 'all')
  const setShow = (s: Show) => {
    shownFor.set(name, s)
    setShowState(s)
  }
  // If the chosen filter no longer has rows, fall back to every song.
  const split = missing > 0 || online.length > 0
  const view: Show = !split
    ? 'all'
    : show === 'get' && !missing
      ? 'all'
      : show === 'online' && !online.length
        ? 'all'
        : show === 'here' && !have.length
          ? 'all'
          : show
  const position = useMemo(() => new Map((rows ?? []).map((r, i) => [r.key, i])), [rows])
  const visible = useMemo(
    () =>
      (rows ?? []).filter((r) =>
        view === 'here' ? !r.missing : view === 'online' ? isRemote(r) : view === 'get' ? toGet(r) : true,
      ),
    [rows, view],
  )
  const groups = useMemo(() => (view === 'get' ? byAlbum(visible) : []), [view, visible])
  // What the list draws: the songs, or (To get) each album's heading and its songs.
  const lines = useMemo<Line<Row>[]>(
    () =>
      view === 'get'
        ? groups.flatMap((g): Line<Row>[] => [
            {
              kind: 'head',
              key: `h:${g.rows[0].key}`,
              node: (
                <div className="flex w-full min-w-0 items-baseline gap-x-3 px-2">
                  <h3 className="min-w-0 truncate text-[14.5px] font-semibold">{g.album || 'No album given'}</h3>
                  <span className="shrink-0 text-[12.5px] text-ink/50">{[g.artist, plural(g.rows.length, 'song')].filter(Boolean).join(', ')}</span>
                  {g.album && (
                    <button onClick={() => void searchWeb(`${g.artist} ${g.album}`)} className="btn-ghost btn-sm ml-auto shrink-0" title="Look this album up in your browser">
                      Search the web
                    </button>
                  )}
                </div>
              ),
            },
            ...g.rows.map((r): Line<Row> => ({ kind: 'row', key: r.key, row: r })),
          ])
        : visible.map((r) => ({ kind: 'row', key: r.key, row: r })),
    [view, groups, visible],
  )
  // What plays, as the engine picks it: your file first, else online; songs to get left out.
  // `of`: the rows it was read for, so what comes next follows only a queue read for these rows.
  const [queue, setQueue] = useState<{ of: Row[] | null; songs: QueueItem[] }>({ of: null, songs: [] })
  useEffect(() => {
    if (rows === null) return
    let live = true
    playlistQueue(root, name)
      .then((found) => live && setQueue({ of: rows, songs: queueOf(found) }))
      .catch(() => {})
    return () => {
      live = false
    }
  }, [root, name, rows, props.catalog])
  const songs = queue.songs
  const totalMs = [...have, ...online].reduce((s, r) => s + r.duration_ms, 0)
  const dirs = useMemo(() => {
    const out: string[] = []
    for (const r of have) {
      const d = r.path.slice(0, r.path.lastIndexOf('/'))
      if (!out.includes(d)) out.push(d)
      if (out.length === 4) break
    }
    return out
  }, [have])
  const glow = useCovers(320)(dirs[0] ?? '')?.src

  const nowKey = usePlayerValue((s) => {
    const c = current(s)
    return c ? queueKey(c) : ''
  })
  const nowFrom = usePlayerValue((s) => s.from)
  const playing = usePlayerValue((s) => s.playing)
  const isNow = fromIs(nowFrom, 'playlist', name)
  const from = useMemo(() => playlistFrom(name), [name])

  const playFrom = async (row: Row) => {
    if (rowKey(row) === nowKey && isNow) return void toggle()
    // Read just now (before the queue came): ask for it.
    const list = songs.length ? songs : queueOf(await playlistQueue(root, name))
    const i = list.findIndex((q) => queueKey(q) === rowKey(row))
    void playItems(root, list, Math.max(0, i), from)
  }
  // Changed while it plays (added, removed, moved, refilled by its rules): what comes next follows.
  useEffect(() => {
    if (rows !== null && queue.of === rows) void followItems(root, queue.songs, from)
  }, [root, queue, from, rows])

  async function rename(to: string) {
    const next = to.trim()
    if (!next || next === name) return
    try {
      await playlistRename(root, name, next)
      props.onRenamed(next)
    } catch (e) {
      notify(e instanceof Error ? e.message : String(e))
    }
  }

  async function exportFile() {
    const to = await saveDialog({ defaultPath: `${name}.m3u8`, filters: [{ name: 'Playlist', extensions: ['m3u8'] }] })
    if (!to) return
    try {
      const n = await playlistExport(root, name, to)
      notify(`Saved ${plural(n, 'song')} to ${to}. It points at your files, for other apps on this computer.`)
    } catch (e) {
      notify(`Couldn't export: ${e instanceof Error ? e.message : String(e)}`)
    }
  }

  async function remove() {
    const ok = await confirmAsk({
      title: `Delete “${name}”?`,
      text: 'The playlist file is moved to the Set aside folder beside your music, not destroyed. Your songs stay where they are.',
      confirm: 'Delete playlist',
      danger: true,
    })
    if (!ok) return
    try {
      await playlistDelete(root, name)
    } catch (e) {
      notify(`Couldn't delete ${name}: ${String(e)}`)
      return
    }
    notify(`Deleted ${name}. Its file is in Set aside.`)
    props.onGone()
  }

  /** Four stars for every song here without a rating; it writes the files, so a backup comes first. */
  async function rateUnrated() {
    try {
      const r = await ratePlaylist(root, name, 4)
      notify(r.changed ? `Gave ${plural(r.changed, 'unrated song')} four stars.` : 'Every song here already has a rating.')
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      if (!msg.includes('BACKUP_NOT_CONFIRMED')) return notify(msg)
      const ok = await confirmAsk({
        title: 'Do you have a backup of this library?',
        text: 'Ratings are written into your music files. Every change is journaled and can be undone, but a backup is the real safety net.',
        confirm: 'I have a backup',
      })
      if (!ok) return
      await confirmBackup()
      await rateUnrated()
    }
  }

  async function refind() {
    const n = await playlistRefind(root, name)
    notify(n ? `Found ${plural(n, 'song')} that came into your library since.` : 'None of the missing songs are in your library yet.')
    if (n) {
      await load()
      props.onChanged()
    }
  }

  const more = useContextMenu(() => [
    ['Play next', () => void enqueueItems(root, songs, true)],
    ['Add to queue', () => void enqueueItems(root, songs, false)],
    ['Add all to another playlist…', () => askAddSongs([...have, ...online].map(songOf), name)],
    ...((smart
      ? [['Duplicate as an ordinary playlist', () => void playlistDuplicate(root, name).then((n) => (props.onChanged(), props.onOpen(n)))]]
      : [
          ['Rename', () => setRenameAsk((n) => n + 1)],
          ['Duplicate', () => void playlistDuplicate(root, name).then((n) => (props.onChanged(), props.onOpen(n)))],
          ...(props.summary?.source_id && props.onUpdate
            ? [[`Update from ${fromWhere(props.summary.source ?? '') || 'where it came from'}…`, () => props.onUpdate!(name)]]
            : []),
          ...(missing ? [['Look for missing songs again', () => void refind()]] : []),
          ...(missing ? [['Fill missing online…', () => setPicking({ rows: (rows ?? []).filter(toGet), mode: 'use' })]] : []),
        ]) as [string, () => void][]),
    ...((have.length ? [['Give unrated songs four stars', () => void rateUnrated()]] : []) as [string, () => void][]),
    ['Export as a file…', () => void exportFile()],
    ...((smart ? [] : [['Delete…', () => void remove()]]) as [string, () => void][]),
  ])

  /** "Yes, that's it": the song takes its place. */
  const takeMaybe = (r: Row) => {
    const m = r.maybe
    if (!m) return
    const before = latest.current
    const next = before.map((x) =>
      x.key === r.key
        ? { ...x, path: m.path, title: m.title, artist: m.artist, album: m.album, duration_ms: m.duration_ms, missing: false, want: undefined, maybe: undefined, online: undefined }
        : x,
    )
    void persist(next, `“${m.title}” is in its place now.`, before)
  }
  /** "Not this one": remembered in the playlist, so it isn't offered again. */
  const refuseMaybe = (r: Row) => {
    const m = r.maybe
    if (!m) return
    const want = { ...(r.want ?? { title: r.title, artist: r.artist, album: r.album, duration_ms: r.duration_ms }) }
    want.not = [...(want.not ?? []), m.path]
    const before = latest.current
    void persist(before.map((x) => (x.key === r.key ? { ...x, want, maybe: undefined } : x)))
  }

  /** Songs to get become the online songs you picked for them, in their places: one save,
   *  one Undo (back to songs to get). Only ids are kept, never a stream address. */
  const useOnlineHere = (chosen: [Row, OnlineEntry][]) => {
    const pick = new Map(chosen.map(([r, o]) => [r.key, o]))
    const before = latest.current
    const next = before.map((x) => {
      const o = pick.get(x.key)
      return o
        ? {
            ...x,
            path: '',
            title: o.title,
            artist: (o.artists ?? []).join(', '),
            album: o.album ?? '',
            duration_ms: o.duration_ms ?? 0,
            missing: true,
            want: undefined,
            maybe: undefined,
            online: o,
          }
        : x
    })
    const one = chosen.length === 1 ? chosen[0][1] : null
    void persist(next, one ? `“${one.title}” plays online from ${name} now.` : `${plural(chosen.length, 'song')} play online from ${name} now.`, before)
  }

  const rowFor = (r: Row, style: React.CSSProperties, grab: Grab | null, inGroup: boolean) => (
    <PlaylistRow
      key={r.key}
      row={r}
      index={position.get(r.key) ?? 0}
      smart={smart}
      style={style}
      grab={grab}
      inGroup={inGroup}
      state={rowKey(r) && rowKey(r) === nowKey && isNow ? (playing ? 'playing' : 'paused') : 'no'}
      badge={mixed}
      onPlay={() => void playFrom(r)}
      onRemove={() => {
        const before = latest.current
        void persist(
          before.filter((x) => x.key !== r.key),
          `Removed “${r.title}” from ${name}.`,
          before,
        )
      }}
      onFind={() => setFinding(r)}
      onYes={() => takeMaybe(r)}
      onNo={() => refuseMaybe(r)}
      onAlbum={() => r.path && props.onAlbum(r.path.slice(0, r.path.lastIndexOf('/')))}
      onAddTo={() => (r.online ? askAddSongs([songOf(r)], r.title) : r.path && askAddToPlaylist([r.path], r.title))}
      onPickOnline={(mode) => setPicking({ rows: [r], mode })}
      root={root}
    />
  )

  const kind = smart
    ? 'Smart playlist'
    : props.summary?.source
      ? `From ${fromWhere(props.summary.source)}${sourceDate(props.summary.source) ? `, brought over ${sourceDate(props.summary.source)}` : ''}`
      : 'Playlist'

  return (
    <Detail
      title={name}
      onBack={props.onBack}
      glow={glow}
      tools={
        songs.length > 0 ? (
          <button onClick={() => (isNow ? void toggle() : void playAllItems(root, songs, from))} className="btn-primary btn-sm">
            {isNow && playing ? <IconPause width={15} height={15} /> : <IconPlay width={15} height={15} />}
            {isNow && playing ? 'Pause' : 'Play'}
          </button>
        ) : undefined
      }
    >
      {more.menu}
      <div className="flex flex-wrap items-end gap-8">
        <Mosaic root={root} dirs={dirs} smart={smart} className="aspect-square w-[220px] shrink-0 rounded-3xl shadow-2xl shadow-black/40" />
        <div className="min-w-[280px] flex-1 pb-1">
          <p className="text-[13px] font-medium text-ink/60">{kind}</p>
          <Title
            name={name}
            editable={!smart}
            startEditing={!!props.fresh || !!props.rename}
            ask={renameAsk}
            onRename={rename}
            onDone={() => props.fresh && have.length === 0 && setAdding(true)}
          />
          <p className="mt-2 text-[13.5px] text-ink/60">
            {rows === null
              ? 'Reading…'
              : missing > 0 || online.length > 0
                ? `${plural(rows.length, 'song')}: ${[
                    `${have.length.toLocaleString('en-IN')} in your library`,
                    online.length ? `${online.length.toLocaleString('en-IN')} online` : '',
                    missing ? `${missing.toLocaleString('en-IN')} to get` : '',
                  ]
                    .filter(Boolean)
                    .join(', ')}`
                : plural(have.length, 'song')}
            {have.length + online.length > 0 && `, ${length(totalMs)}${missing > 0 ? ' to play' : ''}`}
          </p>
          {rows !== null && missing > 0 && (
            <div
              className="mt-2.5 h-1 max-w-[320px] overflow-hidden rounded-full bg-ink/[0.1]"
              role="meter"
              aria-label="Songs in your library"
              aria-valuemin={0}
              aria-valuemax={rows.length}
              aria-valuenow={have.length}
            >
              <div className="h-full rounded-full bg-accent transition-[width] duration-500" style={{ width: `${(have.length / Math.max(1, rows.length)) * 100}%` }} />
            </div>
          )}
          {smart && props.rule && <p className="mt-1 max-w-[70ch] text-[12.5px] leading-relaxed text-ink/50">{describeRule(props.rule)}</p>}
          <div className="mt-5 flex flex-wrap items-center gap-2.5">
            <button
              disabled={!songs.length}
              onClick={() => (isNow ? void toggle() : void playAllItems(root, songs, from))}
              className="btn-primary btn-lg play-main"
            >
              {isNow && playing ? <IconPause /> : <IconPlay />}
              {isNow ? (playing ? 'Pause' : 'Resume') : 'Play'}
            </button>
            <button disabled={songs.length < 2} onClick={() => void playAllItems(root, songs, from, 'shuffle')} className="btn btn-lg shuffle-main">
              <IconShuffle />
              Shuffle
            </button>
            {smart ? (
              <button onClick={() => setRules((o) => !o)} aria-expanded={rules} className="btn-ghost btn-lg">
                {rules ? 'Hide rules' : 'Edit rules'}
              </button>
            ) : (
              <button onClick={() => setAdding((o) => !o)} aria-expanded={adding} className="btn-ghost btn-lg">
                {adding ? 'Done adding' : 'Add songs'}
              </button>
            )}
            <button onClick={more.onContextMenu} aria-label="More for this playlist" title="More" className="grid h-10 w-10 place-items-center rounded-full text-ink/70 transition-colors hover:bg-ink/10 hover:text-ink active:scale-90">
              <IconMore />
            </button>
          </div>
        </div>
      </div>

      <AnimatePresence>
        {rules && props.rule && (
          <SmartRules
            key="rules"
            root={root}
            rule={props.rule}
            onClose={() => setRules(false)}
            onSaved={() => {
              setRules(false)
              props.onChanged()
              void load()
              notify('Saved. The playlist was filled again from the rules.')
            }}
          />
        )}
        {adding && !smart && (
          <SongFinder
            key="add"
            title="Add songs"
            hint="Find songs in your library or online; each one you pick goes to the end."
            catalog={props.catalog}
            excludeLocal={new Set(have.map((r) => r.path))}
            excludeOnline={new Set(online.map((r) => (r.online ? `${r.online.provider}:${r.online.track_id}` : '')).filter(Boolean))}
            onPickLocal={(t) => {
              const before = latest.current
              void persist([
                ...before,
                ...withKeys([
                  {
                    path: t.path,
                    title: t.title,
                    artist: t.artists.join(', '),
                    album: t.album,
                    duration_ms: t.duration_ms,
                    audio_md5: null,
                    missing: false,
                  },
                ]),
              ])
            }}
            onPickOnline={(t) => {
              const before = latest.current
              const o = onlineEntryFrom(t)
              void persist([
                ...before,
                ...withKeys([
                  {
                    path: '',
                    title: o.title,
                    artist: (o.artists ?? []).join(', '),
                    album: o.album ?? '',
                    duration_ms: o.duration_ms ?? 0,
                    audio_md5: null,
                    missing: true,
                    online: o,
                  },
                ]),
              ])
            }}
            onClose={() => setAdding(false)}
          />
        )}
      </AnimatePresence>

      {problem && <p className="mt-8 text-[13px] text-problem">{problem}</p>}

      {rows !== null && rows.length === 0 && !problem && !adding && (
        <div className="mt-10 rounded-2xl border border-dashed border-edge px-6 py-12 text-center">
          <p className="text-[15px] font-medium">{smart ? 'No songs match these rules yet.' : 'This playlist is empty.'}</p>
          <p className="mt-1.5 text-[13px] text-ink/55">
            {smart
              ? 'Change the rules, or rate some songs.'
              : 'Use Add songs above, or "Add to playlist…" on any song or album (right-click it).'}
          </p>
        </div>
      )}

      {rows && (missing > 0 || online.length > 0) && !smart && (
        <div className="mt-8 flex flex-wrap items-center gap-x-4 gap-y-2">
          <Segmented
            tabs
            size="sm"
            label="Which songs"
            value={view}
            onChange={setShow}
            options={[
              { value: 'all', label: `All ${rows.length.toLocaleString('en-IN')}` },
              {
                value: 'here',
                label: `Local ${have.length.toLocaleString('en-IN')}`,
                disabled: !have.length,
              },
              ...(online.length
                ? [
                    {
                      value: 'online' as const,
                      label: `Online ${online.length.toLocaleString('en-IN')}`,
                    },
                  ]
                : []),
              ...(missing
                ? [{ value: 'get' as const, label: `To get ${missing.toLocaleString('en-IN')}` }]
                : []),
            ]}
          />
          {view === 'get' && (
            <span className="text-[12.5px] text-ink/50">
              From {plural(groups.length, 'album')}, the ones with most songs first. Venuta never downloads music.
            </span>
          )}
          {view === 'online' && (
            <span className="text-[12.5px] text-ink/50">Plays from the online catalog until you have the file.</span>
          )}
          {view === 'get' && missing > 0 && !smart && (
            <button
              onClick={() => setPicking({ rows: (rows ?? []).filter(toGet), mode: 'use' })}
              className="btn-ghost btn-sm ml-auto"
              title="Choose an online song for each song to get, one at a time"
            >
              Fill missing online…
            </button>
          )}
        </div>
      )}

      {rows && rows.length > 0 && (
        <div className={(missing > 0 || online.length > 0) && !smart ? 'mt-4' : 'mt-9'}>
          <WindowedList
            lines={lines}
            rowHeight={64}
            headHeight={52}
            label={name}
            movable={view === 'all' && !smart}
            onMove={(from, to) => {
              const next = latest.current.slice()
              const [moved] = next.splice(from, 1)
              next.splice(to, 0, moved)
              void persist(next)
            }}
            render={(r, _i, style, grab) => rowFor(r, style, grab, view === 'get')}
          />
        </div>
      )}

      <AnimatePresence>
        {picking && (
          <OnlinePick
            key="online-pick"
            root={root}
            rows={picking.rows}
            mode={picking.mode}
            onUse={useOnlineHere}
            onClose={() => setPicking(null)}
          />
        )}
        {finding && (
          <FindDialog
            row={finding}
            catalog={props.catalog}
            onClose={() => setFinding(null)}
            onPick={(t) => {
              const before = latest.current
              const next = before.map((x) =>
                x.key === finding.key
                  ? { ...x, path: t.path, title: t.title, artist: t.artists.join(', '), album: t.album, duration_ms: t.duration_ms, missing: false, want: undefined, maybe: undefined, online: undefined }
                  : x,
              )
              setFinding(null)
              void persist(next, `“${t.title}” is in its place now.`, before)
            }}
          />
        )}
      </AnimatePresence>
    </Detail>
  )
}

/** The playlist's name: click to rename (Enter keeps it, Escape doesn't). */
function Title(props: { name: string; editable: boolean; startEditing: boolean; ask: number; onRename: (to: string) => void; onDone: () => void }) {
  const [editing, setEditing] = useState(props.startEditing)
  const [value, setValue] = useState(props.name)
  useEffect(() => setValue(props.name), [props.name])
  useEffect(() => {
    if (props.ask) setEditing(true)
  }, [props.ask])
  if (!editing || !props.editable)
    return (
      <h1
        className={`title hero-title mt-1 text-[34px] leading-[1.1] ${props.editable ? 'cursor-text rounded-lg decoration-ink/30 underline-offset-8 hover:underline' : ''}`}
        onClick={() => props.editable && setEditing(true)}
        title={props.editable ? 'Click to rename' : undefined}
      >
        {props.name}
      </h1>
    )
  return (
    <input
      autoFocus
      onFocus={(e) => e.currentTarget.select()}
      value={value}
      aria-label="Playlist name"
      onChange={(e) => setValue(e.target.value)}
      onBlur={() => {
        setEditing(false)
        props.onRename(value)
        props.onDone()
      }}
      onKeyDown={(e) => {
        if (e.key === 'Enter') e.currentTarget.blur()
        if (e.key === 'Escape') {
          e.preventDefault()
          setValue(props.name)
          setEditing(false)
          props.onDone()
        }
      }}
      className="title mt-1 w-full rounded-lg bg-ink/[0.06] px-2 py-0.5 text-[34px] leading-[1.1] outline-none ring-1 ring-[color-mix(in_oklab,var(--accent)_60%,transparent)] -mx-2"
    />
  )
}

function PlaylistRow(props: {
  root: string
  row: Row
  /** Its place in the playlist (shown as its number, whatever is filtered). */
  index: number
  smart: boolean
  /** Where the list puts it. */
  style: React.CSSProperties
  /** Moving it (null: not here, a smart playlist or a filtered view). */
  grab: Grab | null
  /** Under its album's heading (To get): the album isn't repeated on the row. */
  inGroup: boolean
  state: 'no' | 'playing' | 'paused'
  /** Say where it plays from (the playlist holds online songs). */
  badge: boolean
  onPlay: () => void
  onRemove: () => void
  onFind: () => void
  onYes: () => void
  onNo: () => void
  onAlbum: () => void
  onAddTo: () => void
  /** Pick an online song for this to-get row: to use in its place, or just to play. */
  onPickOnline: (mode: 'use' | 'play') => void
}) {
  const { row: r, state, grab } = props
  const cover = useCovers(320)(r.path ? r.path.slice(0, r.path.lastIndexOf('/')) : '')
  const isNow = state !== 'no'
  // Still to get (not here, not online), or an online song that plays as it is.
  const get = toGet(r)
  const remote = isRemote(r)
  const onlineArt = r.online?.artwork_url
  const removable = (props.smart ? [] : [['Remove from playlist', props.onRemove]]) as [string, () => void][]
  const ctx = useContextMenu(() =>
    remote && r.online
      ? [
          ['Play', props.onPlay],
          ['Play next', () => void enqueueItems(props.root, [onlineItem(r.online!)], true)],
          ['Add to queue', () => void enqueueItems(props.root, [onlineItem(r.online!)], false)],
          ['Add to another playlist…', props.onAddTo],
          ['Find it in your library…', props.onFind],
          ...removable,
        ]
      : get
      ? [
          ...((r.maybe ? [[`Use “${r.maybe.title}” from your library`, props.onYes]] : []) as [string, () => void][]),
          ['Play online…', () => props.onPickOnline('play')],
          ['Use online in this playlist…', () => props.onPickOnline('use')],
          ['Find it in your library…', props.onFind],
          [`Copy “${wordsFor(r) || r.title}”`, () => void navigator.clipboard.writeText(wordsFor(r) || r.title).then(() => notify('Copied.'))],
          ['Search the web', () => void searchWeb(`${wordsFor(r)} ${r.album}`.trim())],
          ...removable,
        ]
      : [
          ['Play', props.onPlay],
          ['Play next', () => void enqueue(props.root, tracksFor([r.path]), true)],
          ['Add to queue', () => void enqueue(props.root, tracksFor([r.path]), false)],
          ['Add to another playlist…', props.onAddTo],
          ['Go to album', props.onAlbum],
          ...removable,
        ],
  )
  return (
    <div
      role="listitem"
      style={props.style}
      aria-label={get ? `${r.title}${r.artist ? `, ${r.artist}` : ''}: not in your library` : undefined}
      tabIndex={0}
      onContextMenu={ctx.onContextMenu}
      onClick={(e) => {
        if (grab?.justDropped() || (e.target as HTMLElement).closest('button')) return
        if (!get) props.onPlay()
      }}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return
        if (grab && e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
          e.preventDefault()
          grab.nudge(e.key === 'ArrowUp' ? -1 : 1)
          return
        }
        if (e.key === 'Enter') {
          if (get) props.onFind()
          else props.onPlay()
        }
        if (e.key === 'Delete' && !props.smart) props.onRemove()
      }}
      className={`row group absolute inset-x-0 grid grid-cols-[22px_32px_44px_minmax(0,1fr)_minmax(0,0.8fr)_52px_32px] items-center gap-3 rounded-xl px-2 transition-colors ${
        get ? 'cursor-default hover:bg-ink/[0.03]' : 'cursor-pointer hover:bg-ink/[0.05] active:bg-ink/[0.09]'
      } ${isNow ? 'text-accent-ink' : ''} ${grab?.dragging ? 'scale-[1.01] bg-[var(--popover)] shadow-[0_14px_32px_-12px_rgb(0_0_0/0.55)]' : ''}`}
    >
      {ctx.menu}
      {!grab ? (
        <span />
      ) : (
        <span
          onPointerDown={grab.onGrab}
          className={`grid h-8 cursor-grab touch-none place-items-center text-ink/25 transition-opacity hover:text-ink/70 active:cursor-grabbing group-hover:opacity-100 ${grab.dragging ? 'opacity-100' : 'opacity-0'}`}
          title="Drag to move (or Alt+Up, Alt+Down)"
          aria-hidden
        >
          <svg width="12" height="16" viewBox="0 0 12 16" fill="currentColor">
            {[2, 7, 12].map((y) => (
              <g key={y}>
                <circle cx="3" cy={y} r="1.3" />
                <circle cx="9" cy={y} r="1.3" />
              </g>
            ))}
          </svg>
        </span>
      )}
      <span className="relative grid h-8 place-items-center text-[13px] tabular-nums text-ink/45">
        {isNow ? (
          <>
            <span className="group-hover:hidden">
              <Equalizer playing={state === 'playing'} />
            </span>
            <span className="hidden text-ink group-hover:block">{state === 'playing' ? <IconPause width={15} height={15} /> : <IconPlay width={15} height={15} />}</span>
          </>
        ) : get ? (
          props.index + 1
        ) : (
          <>
            <span className="group-hover:hidden">{props.index + 1}</span>
            <span className="hidden text-ink group-hover:block">
              <IconPlay width={15} height={15} />
            </span>
          </>
        )}
      </span>
      {get ? (
        // Not here: an empty sleeve, marked, so it reads at a glance without colour alone.
        <span className="relative grid h-11 w-11 place-items-center rounded-lg border border-dashed border-ink/25 text-ink/35" aria-hidden>
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round">
            <circle cx="12" cy="12" r="8.5" />
            <circle cx="12" cy="12" r="1.6" />
          </svg>
          <span className="absolute -bottom-1 -right-1 grid h-4 w-4 place-items-center rounded-full bg-[var(--popover)] text-[10px] font-bold leading-none text-ink/70 shadow-[inset_0_0_0_1px_var(--edge)]">
            +
          </span>
        </span>
      ) : remote ? (
        <span className="grid h-11 w-11 place-items-center overflow-hidden rounded-lg bg-ink/[0.07] text-ink/30">
          <RemoteArt url={onlineArt} fallback="♪" imgClassName="h-full w-full object-cover" />
        </span>
      ) : (
        <span className="h-11 w-11 overflow-hidden rounded-lg bg-ink/[0.07]">
          {cover ? <img src={cover.src} alt="" className="h-full w-full object-cover" draggable={false} /> : <span className="grid h-full w-full place-items-center text-ink/30">♪</span>}
        </span>
      )}
      <span className="min-w-0">
        <span className="flex min-w-0 items-center gap-1.5">
          <span className={`truncate text-[14px] font-medium ${get ? 'text-ink/85' : ''}`}>{r.title}</span>
          {props.badge && <AvailabilityBadge availability={entryAvailability(r)} />}
        </span>
        <span className={`block truncate text-[12.5px] ${isNow ? 'opacity-80' : 'text-ink/55'}`}>{r.artist || '—'}</span>
      </span>
      <span className="min-w-0 text-[13px] text-ink/55">
        {get && r.maybe ? (
          <span className="flex min-w-0 items-center gap-1.5">
            <span className="min-w-0 flex-1 truncate" title={`${r.maybe.title}, ${r.maybe.artist}, ${r.maybe.album} (${r.maybe.why})`}>
              <span className="text-ink/45">Maybe </span>
              <span className="text-ink/80">{r.maybe.title}</span>
              <span className="text-ink/45">, {r.maybe.album}</span>
            </span>
            <button onClick={props.onYes} className="btn-sm btn shrink-0" title={`Use “${r.maybe.title}” from your library here (${r.maybe.why})`}>
              Yes
            </button>
            <button onClick={props.onNo} className="btn-ghost btn-sm shrink-0" title="It isn't this song; don't offer it again">
              No
            </button>
          </span>
        ) : get ? (
          <span className="flex min-w-0 items-center gap-2">
            <span className="min-w-0 flex-1">
              {r.album && !props.inGroup && <span className="block truncate text-ink/55">{r.album}</span>}
              <span className="block truncate text-[12px] text-ink/40">
                <span className="hidden lg:inline">Not in your library</span>
                <span className="lg:hidden">To get</span>
              </span>
            </span>
            <button onClick={props.onFind} className="btn-ghost btn-sm shrink-0 opacity-0 transition-opacity focus-visible:opacity-100 group-hover:opacity-100" title="Pick the song in your library">
              Find
            </button>
          </span>
        ) : (
          <span className="block truncate">{r.album}</span>
        )}
      </span>
      <span className="text-right text-[12.5px] tabular-nums text-ink/55">{r.duration_ms ? clock(r.duration_ms) : ''}</span>
      {props.smart || get ? (
        <button onClick={ctx.onContextMenu} aria-label={`More for ${r.title}`} className="grid h-8 w-8 place-items-center rounded-full text-ink/55 opacity-0 transition-opacity hover:bg-ink/10 hover:text-ink focus-visible:opacity-100 group-hover:opacity-100">
          <IconMore />
        </button>
      ) : (
        <button
          onClick={props.onRemove}
          aria-label={`Remove ${r.title} from the playlist`}
          title="Remove from playlist"
          className="grid h-8 w-8 place-items-center rounded-full text-[17px] leading-none text-ink/45 opacity-0 transition-[opacity,background-color] hover:bg-ink/10 hover:text-ink focus-visible:opacity-100 active:scale-90 group-hover:opacity-100"
        >
          ×
        </button>
      )}
    </div>
  )
}

/** Search your library for songs, by any words of their title, artists or album. */
function useSongSearch(q: string, catalog: CatalogTrack[] | null, limit = 40): CatalogTrack[] {
  return useMemo(() => {
    if (!q.trim()) return []
    const has = matcher(q)
    const out: CatalogTrack[] = []
    for (const t of catalog ?? []) {
      if (has(keyFor(t, () => `${t.title} ${t.artists.join(' ')} ${t.album}`))) {
        out.push(t)
        if (out.length >= limit) break
      }
    }
    return out
  }, [q, catalog, limit])
}

type FinderSource = 'library' | 'online'

/** Add songs from your library or the online catalog (same picker, two sources). */
function SongFinder(props: {
  title: string
  hint: string
  catalog: CatalogTrack[] | null
  excludeLocal: Set<string>
  excludeOnline: Set<string>
  onPickLocal: (t: CatalogTrack) => void
  onPickOnline: (t: OnlineTrack) => void
  onClose: () => void
}) {
  const [q, setQ] = useState('')
  const [source, setSource] = useState<FinderSource>('library')
  const [sel, setSel] = useState(0)
  const found = useSongSearch(q, props.catalog)
  const cover = useCovers(320)
  const { results: online, error: onlineError, loading: onlineLoading, resultsFor } = useOnlineSearch(q)
  const onlineTracks = online?.tracks ?? []
  const onlineFresh = resultsFor === q.trim()
  const listLen = source === 'library' ? found.length : onlineTracks.length
  useEffect(() => setSel(0), [q, source, listLen])
  useEscape(true, props.onClose)
  const pickSel = () => {
    if (source === 'library') {
      const t = found[sel]
      if (t && !props.excludeLocal.has(t.path)) props.onPickLocal(t)
    } else {
      const t = onlineTracks[sel]
      if (!t) return
      const id = `${t.provider}:${t.track_id}`
      if (!props.excludeOnline.has(id)) props.onPickOnline(t)
    }
  }
  return (
    <motion.section
      initial={{ opacity: 0, y: -6 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -6 }}
      className="glass mt-8 rounded-[20px] p-4"
      aria-label={props.title}
    >
      <div className="mb-3 flex flex-wrap items-center gap-3">
        <Segmented
          size="sm"
          label="Where to find songs"
          value={source}
          onChange={setSource}
          options={[
            { value: 'library', label: 'In your library' },
            { value: 'online', label: 'Online' },
          ]}
        />
        <span className="text-[12.5px] text-ink/50">
          {source === 'library' ? 'Your files' : 'Plays online until you have the file'}
        </span>
      </div>
      <div className="flex items-center gap-3 rounded-full bg-ink/[0.07] px-4 shadow-[inset_0_0_0_1px_var(--edge)] focus-within:bg-ink/[0.1]">
        <IconSearch className="shrink-0 text-ink/45" />
        <input
          autoFocus
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Escape' && q) {
              e.preventDefault()
              setQ('')
              return
            }
            if (e.key === 'ArrowDown') {
              e.preventDefault()
              setSel((i) => Math.min(i + 1, Math.max(0, listLen - 1)))
            } else if (e.key === 'ArrowUp') {
              e.preventDefault()
              setSel((i) => Math.max(i - 1, 0))
            } else if (e.key === 'Enter') {
              e.preventDefault()
              pickSel()
            }
          }}
          placeholder={source === 'library' ? 'Find a song, an artist or an album' : 'Search online songs'}
          aria-label={props.title}
          className="min-w-0 flex-1 bg-transparent py-2.5 text-[14px] outline-none placeholder:text-ink/45"
        />
        {source === 'online' && onlineLoading && (
          <span className="spin h-3.5 w-3.5 shrink-0 rounded-full border-2 border-ink/20 border-t-ink/60" aria-label="Searching online" />
        )}
      </div>
      {!q && (
        <p className="px-2 pt-3 text-[12.5px] text-ink/50">
          {source === 'library' ? (props.catalog ? props.hint : 'Reading your library…') : 'Type a few words to search the online catalog.'}
        </p>
      )}
      {source === 'library' && q && !found.length && (
        <p className="px-2 pt-3 text-[12.5px] text-ink/50">
          Nothing in your library matches “{q}”. Try the Online tab to add it without the file.
        </p>
      )}
      {source === 'online' && q && onlineError && onlineFresh && !onlineLoading && (
        <p className="px-2 pt-3 text-[12.5px] text-ink/55">{onlineError}</p>
      )}
      {source === 'online' && q && onlineFresh && !onlineLoading && !onlineError && !onlineTracks.length && (
        <p className="px-2 pt-3 text-[12.5px] text-ink/50">Nothing online matches “{q}”.</p>
      )}
      <div className="fade-y mt-2 grid max-h-[340px] gap-0.5 overflow-y-auto">
        {source === 'library' &&
          found.map((t, i) => {
            const inIt = props.excludeLocal.has(t.path)
            return (
              <button
                key={t.path}
                disabled={inIt}
                onMouseMove={() => setSel(i)}
                onClick={() => props.onPickLocal(t)}
                className={`flex items-center gap-3 rounded-xl px-2 py-1.5 text-left transition-colors enabled:hover:bg-ink/[0.06] enabled:active:bg-ink/[0.1] disabled:opacity-50 ${
                  i === sel ? 'bg-ink/[0.07]' : ''
                }`}
              >
                <span className="h-9 w-9 shrink-0 overflow-hidden rounded-md bg-ink/[0.07]">
                  {cover(t.dir) && <img src={cover(t.dir)!.src} alt="" className="h-full w-full object-cover" />}
                </span>
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[13.5px]">{t.title}</span>
                  <span className="block truncate text-[12px] text-ink/55">
                    {t.artists.join(', ')} · {t.album}
                  </span>
                </span>
                <span className="shrink-0 text-[12px] text-ink/55">{inIt ? 'In the playlist' : '+ Add'}</span>
              </button>
            )
          })}
        {source === 'online' && onlineLoading && !onlineTracks.length && (
          <div className="grid gap-1.5 px-1 py-2" aria-busy="true">
            {Array.from({ length: 4 }, (_, i) => (
              <div key={i} className="flex items-center gap-3 px-1">
                <span className="shimmer h-9 w-9 rounded-md" />
                <span className="min-w-0 flex-1 grid gap-1.5">
                  <span className="shimmer h-3 w-2/3 rounded-full" />
                  <span className="shimmer h-2.5 w-1/2 rounded-full" />
                </span>
              </div>
            ))}
          </div>
        )}
        {source === 'online' &&
          onlineTracks.map((t, i) => {
            const id = `${t.provider}:${t.track_id}`
            const inIt = props.excludeOnline.has(id)
            return (
              <button
                key={id}
                disabled={inIt}
                onMouseMove={() => setSel(i)}
                onClick={() => props.onPickOnline(t)}
                className={`flex items-center gap-3 rounded-xl px-2 py-1.5 text-left transition-colors enabled:hover:bg-ink/[0.06] enabled:active:bg-ink/[0.1] disabled:opacity-50 ${
                  i === sel ? 'bg-ink/[0.07]' : ''
                }`}
              >
                <span className="grid h-9 w-9 shrink-0 place-items-center overflow-hidden rounded-md bg-ink/[0.07] text-ink/30">
                  <RemoteArt url={t.artwork_url} fallback="♪" imgClassName="h-full w-full object-cover" />
                </span>
                <span className="min-w-0 flex-1">
                  <span className="flex min-w-0 items-center gap-1.5">
                    <span className="truncate text-[13.5px]">{t.title}</span>
                    <AvailabilityBadge availability="online" />
                  </span>
                  <span className="block truncate text-[12px] text-ink/55">
                    {t.artists.join(', ') || '—'}
                    {t.album ? ` · ${t.album}` : ''}
                  </span>
                </span>
                <span className="shrink-0 text-[12px] text-ink/55">{inIt ? 'In the playlist' : '+ Add'}</span>
              </button>
            )
          })}
      </div>
    </motion.section>
  )
}

/** Pick the library song a missing entry should be. Starts with what the playlist knew. */
function FindDialog(props: { row: Row; catalog: CatalogTrack[] | null; onPick: (t: CatalogTrack) => void; onClose: () => void }) {
  const [q, setQ] = useState(`${props.row.title}`)
  const found = useSongSearch(q, props.catalog, 30)
  const cover = useCovers(320)
  useEscape(true, props.onClose)
  return (
    <motion.div
      className="fixed inset-0 z-50 flex items-start justify-center bg-[var(--scrim)] pt-[14vh]"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onMouseDown={props.onClose}
    >
      <motion.div
        role="dialog"
        aria-label="Find this song"
        className="popover flex max-h-[64vh] w-[min(520px,92vw)] flex-col overflow-hidden rounded-[18px]"
        initial={{ y: -10, opacity: 0 }}
        animate={{ y: 0, opacity: 1 }}
        exit={{ y: -6, opacity: 0 }}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="border-b border-edge px-4 pb-3 pt-4">
          <div className="text-[12.5px] text-ink/55">
            The playlist wants “{props.row.title}”{props.row.artist ? ` by ${props.row.artist}` : ''}
            {props.row.album ? ` (${props.row.album})` : ''}. Which song in your library is it?
          </div>
          <input
            autoFocus
            onFocus={(e) => e.currentTarget.select()}
            value={q}
            onChange={(e) => setQ(e.target.value)}
            data-escape="layer"
            aria-label="Find the song"
            className="mt-1.5 w-full bg-transparent text-[16px] outline-none placeholder:text-ink/40"
            placeholder="Song, artist or album"
          />
        </div>
        <div className="fade-y min-h-0 flex-1 overflow-y-auto p-1.5">
          {!found.length && <p className="px-3 py-6 text-center text-[13px] text-ink/55">Nothing matches. Try fewer words, or the artist's name.</p>}
          {found.map((t) => (
            <button key={t.path} onClick={() => props.onPick(t)} className="flex w-full items-center gap-3 rounded-xl px-2.5 py-2 text-left transition-colors hover:bg-ink/[0.07] active:bg-ink/[0.1]">
              <span className="h-10 w-10 shrink-0 overflow-hidden rounded-lg bg-ink/[0.07]">
                {cover(t.dir) && <img src={cover(t.dir)!.src} alt="" className="h-full w-full object-cover" />}
              </span>
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[14px]">{t.title}</span>
                <span className="block truncate text-[12px] text-ink/55">
                  {t.artists.join(', ')} · {t.album} · {clock(t.duration_ms)}
                </span>
              </span>
            </button>
          ))}
        </div>
      </motion.div>
    </motion.div>
  )
}

