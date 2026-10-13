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
  playlistRefind,
  playlistRename,
  playlistSaveItems,
  playlistTracks,
  ratePlaylist,
  searchWeb,
  smartSave,
  type CatalogTrack,
  type PlaylistEntry,
  type PlaylistItem,
  type PlaylistSummary,
  type SmartRule,
} from '../../lib/engine'
import { clock, enqueue, follow, fromIs, play, playAll, toggle, usePlayerValue, current } from '../../lib/player'
import { notify } from '../../lib/notify'
import { useEscape } from '../../lib/layers'
import { confirmAsk } from '../../components/Confirm'
import { askAddToPlaylist } from '../../components/AddToPlaylist'
import { Mosaic } from '../../components/Mosaic'
import { Detail, Segmented } from '../../components/ui'
import { IconMore, IconPause, IconPlay, IconSearch, IconShuffle } from '../../components/Icons'
import { Equalizer, useContextMenu } from '../listen/parts'
import { fromWhere, length, playlistFrom, plural, sourceDate } from './common'
import { describeRule, SmartRules } from './SmartRules'
import { PlaylistList, type Grab, type Line } from './PlaylistList'

type Row = PlaylistEntry & { key: string }
let keySeq = 0
const withKeys = (entries: PlaylistEntry[]): Row[] => entries.map((e) => ({ ...e, key: `r${++keySeq}` }))
const itemOf = (r: PlaylistEntry): PlaylistItem =>
  r.missing ? { want: r.want ?? { title: r.title, artist: r.artist, album: r.album, duration_ms: r.duration_ms } } : { path: r.path }

/** Which songs the page shows: all of them, the ones here, or the ones to get. */
type Show = 'all' | 'here' | 'get'
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
        if (!props.rule) throw e
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
  const missing = (rows ?? []).length - have.length
  const [show, setShowState] = useState<Show>(() => shownFor.get(name) ?? 'all')
  const setShow = (s: Show) => {
    shownFor.set(name, s)
    setShowState(s)
  }
  // Nothing left to get: back to every song.
  const view: Show = missing ? show : 'all'
  const position = useMemo(() => new Map((rows ?? []).map((r, i) => [r.key, i])), [rows])
  const visible = useMemo(
    () => (rows ?? []).filter((r) => (view === 'here' ? !r.missing : view === 'get' ? r.missing : true)),
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
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const songs = useMemo(() => tracksFor(have.map((r) => r.path)), [have, props.catalog])
  const totalMs = have.reduce((s, r) => s + r.duration_ms, 0)
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

  const nowPath = usePlayerValue((s) => current(s)?.path ?? '')
  const nowFrom = usePlayerValue((s) => s.from)
  const playing = usePlayerValue((s) => s.playing)
  const isNow = fromIs(nowFrom, 'playlist', name)
  const from = useMemo(() => playlistFrom(name), [name])

  const playFrom = (row: Row) => {
    if (row.path === nowPath && isNow) return void toggle()
    const i = have.findIndex((r) => r.key === row.key)
    void play(root, songs, Math.max(0, i), from)
  }
  // Changed while it plays (added, removed, moved, refilled by its rules): what comes next follows.
  useEffect(() => {
    if (rows !== null) void follow(root, songs, from)
  }, [root, songs, from, rows])

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
    await playlistDelete(root, name)
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
    ['Play next', () => void enqueue(root, songs, true)],
    ['Add to queue', () => void enqueue(root, songs, false)],
    ['Add all to another playlist…', () => askAddToPlaylist(have.map((r) => r.path), name)],
    ...((smart
      ? [['Duplicate as an ordinary playlist', () => void playlistDuplicate(root, name).then((n) => (props.onChanged(), props.onOpen(n)))]]
      : [
          ['Rename', () => setRenameAsk((n) => n + 1)],
          ['Duplicate', () => void playlistDuplicate(root, name).then((n) => (props.onChanged(), props.onOpen(n)))],
          ...(props.summary?.source_id && props.onUpdate
            ? [[`Update from ${fromWhere(props.summary.source ?? '') || 'where it came from'}…`, () => props.onUpdate!(name)]]
            : []),
          ...(missing ? [['Look for missing songs again', () => void refind()]] : []),
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
        ? { ...x, path: m.path, title: m.title, artist: m.artist, album: m.album, duration_ms: m.duration_ms, missing: false, want: undefined, maybe: undefined }
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

  const rowFor = (r: Row, style: React.CSSProperties, grab: Grab | null, inGroup: boolean) => (
    <PlaylistRow
      key={r.key}
      row={r}
      index={position.get(r.key) ?? 0}
      smart={smart}
      style={style}
      grab={grab}
      inGroup={inGroup}
      state={r.path && r.path === nowPath && isNow ? (playing ? 'playing' : 'paused') : 'no'}
      onPlay={() => playFrom(r)}
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
      onAddTo={() => r.path && askAddToPlaylist([r.path], r.title)}
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
          <button onClick={() => (isNow ? void toggle() : void playAll(root, songs, from))} className="btn-primary btn-sm">
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
              : missing > 0
                ? `${plural(rows.length, 'song')}: ${have.length.toLocaleString('en-IN')} in your library, ${missing.toLocaleString('en-IN')} to get`
                : plural(have.length, 'song')}
            {have.length > 0 && `, ${length(totalMs)}${missing > 0 ? ' to play' : ''}`}
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
              onClick={() => (isNow ? void toggle() : void playAll(root, songs, from))}
              className="btn-primary btn-lg"
            >
              {isNow && playing ? <IconPause /> : <IconPlay />}
              {isNow ? (playing ? 'Pause' : 'Resume') : 'Play'}
            </button>
            <button disabled={songs.length < 2} onClick={() => void playAll(root, songs, from, 'shuffle')} className="btn btn-lg">
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
            hint="Find songs in your library; each one you pick goes to the end."
            catalog={props.catalog}
            exclude={new Set(have.map((r) => r.path))}
            onPick={(t) => {
              const before = latest.current
              void persist([...before, ...withKeys([{ path: t.path, title: t.title, artist: t.artists.join(', '), album: t.album, duration_ms: t.duration_ms, audio_md5: null, missing: false }])])
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

      {rows && missing > 0 && !smart && (
        <div className="mt-8 flex flex-wrap items-center gap-x-4 gap-y-2">
          <Segmented
            tabs
            size="sm"
            label="Which songs"
            value={view}
            onChange={setShow}
            options={[
              { value: 'all', label: `All ${rows.length.toLocaleString('en-IN')}` },
              { value: 'here', label: `In your library ${have.length.toLocaleString('en-IN')}`, disabled: !have.length },
              { value: 'get', label: `To get ${missing.toLocaleString('en-IN')}` },
            ]}
          />
          {view === 'get' && (
            <span className="text-[12.5px] text-ink/50">
              From {plural(groups.length, 'album')}, the ones with most songs first. Venuta never downloads music.
            </span>
          )}
        </div>
      )}

      {rows && rows.length > 0 && (
        <div className={missing > 0 && !smart ? 'mt-4' : 'mt-9'}>
          <PlaylistList
            lines={lines}
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
        {finding && (
          <FindDialog
            row={finding}
            catalog={props.catalog}
            onClose={() => setFinding(null)}
            onPick={(t) => {
              const before = latest.current
              const next = before.map((x) =>
                x.key === finding.key
                  ? { ...x, path: t.path, title: t.title, artist: t.artists.join(', '), album: t.album, duration_ms: t.duration_ms, missing: false, want: undefined, maybe: undefined }
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
        className={`title mt-1 text-[34px] leading-[1.1] ${props.editable ? 'cursor-text rounded-lg decoration-ink/30 underline-offset-8 hover:underline' : ''}`}
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
  onPlay: () => void
  onRemove: () => void
  onFind: () => void
  onYes: () => void
  onNo: () => void
  onAlbum: () => void
  onAddTo: () => void
}) {
  const { row: r, state, grab } = props
  const cover = useCovers(320)(r.path ? r.path.slice(0, r.path.lastIndexOf('/')) : '')
  const isNow = state !== 'no'
  const removable = (props.smart ? [] : [['Remove from playlist', props.onRemove]]) as [string, () => void][]
  const ctx = useContextMenu(() =>
    r.missing
      ? [
          ...((r.maybe ? [[`Use “${r.maybe.title}” from your library`, props.onYes]] : []) as [string, () => void][]),
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
      aria-label={r.missing ? `${r.title}${r.artist ? `, ${r.artist}` : ''}: not in your library` : undefined}
      tabIndex={0}
      onContextMenu={ctx.onContextMenu}
      onClick={(e) => {
        if (grab?.justDropped() || (e.target as HTMLElement).closest('button')) return
        if (!r.missing) props.onPlay()
      }}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return
        if (grab && e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
          e.preventDefault()
          grab.nudge(e.key === 'ArrowUp' ? -1 : 1)
          return
        }
        if (e.key === 'Enter') {
          if (r.missing) props.onFind()
          else props.onPlay()
        }
        if (e.key === 'Delete' && !props.smart) props.onRemove()
      }}
      className={`group absolute inset-x-0 grid grid-cols-[22px_32px_44px_minmax(0,1fr)_minmax(0,0.8fr)_52px_32px] items-center gap-3 rounded-xl px-2 transition-colors ${
        r.missing ? 'cursor-default hover:bg-ink/[0.03]' : 'cursor-pointer hover:bg-ink/[0.05] active:bg-ink/[0.09]'
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
        ) : r.missing ? (
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
      {r.missing ? (
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
      ) : (
        <span className="h-11 w-11 overflow-hidden rounded-lg bg-ink/[0.07]">
          {cover ? <img src={cover.src} alt="" className="h-full w-full object-cover" draggable={false} /> : <span className="grid h-full w-full place-items-center text-ink/30">♪</span>}
        </span>
      )}
      <span className="min-w-0">
        <span className={`block truncate text-[14px] font-medium ${r.missing ? 'text-ink/85' : ''}`}>{r.title}</span>
        <span className={`block truncate text-[12.5px] ${isNow ? 'opacity-80' : 'text-ink/55'}`}>{r.artist || '—'}</span>
      </span>
      <span className="min-w-0 text-[13px] text-ink/55">
        {r.missing && r.maybe ? (
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
        ) : r.missing ? (
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
      {props.smart || r.missing ? (
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
    const words = q.toLowerCase().split(/\s+/).filter(Boolean)
    if (!words.length) return []
    const out: CatalogTrack[] = []
    for (const t of catalog ?? []) {
      const hay = `${t.title} ${t.artists.join(' ')} ${t.album}`.toLowerCase()
      if (words.every((w) => hay.includes(w))) {
        out.push(t)
        if (out.length >= limit) break
      }
    }
    return out
  }, [q, catalog, limit])
}

function SongFinder(props: { title: string; hint: string; catalog: CatalogTrack[] | null; exclude: Set<string>; onPick: (t: CatalogTrack) => void; onClose: () => void }) {
  const [q, setQ] = useState('')
  const found = useSongSearch(q, props.catalog)
  const cover = useCovers(320)
  useEscape(true, props.onClose)
  return (
    <motion.section
      initial={{ opacity: 0, y: -6 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -6 }}
      className="glass mt-8 rounded-[20px] p-4"
      aria-label={props.title}
    >
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
            }
          }}
          placeholder="Find a song, an artist or an album"
          aria-label={props.title}
          className="min-w-0 flex-1 bg-transparent py-2.5 text-[14px] outline-none placeholder:text-ink/45"
        />
      </div>
      {!q && <p className="px-2 pt-3 text-[12.5px] text-ink/50">{props.catalog ? props.hint : 'Reading your library…'}</p>}
      {q && !found.length && <p className="px-2 pt-3 text-[12.5px] text-ink/50">Nothing in your library matches “{q}”.</p>}
      <div className="fade-y mt-2 grid max-h-[340px] gap-0.5 overflow-y-auto">
        {found.map((t) => {
          const inIt = props.exclude.has(t.path)
          return (
            <button
              key={t.path}
              disabled={inIt}
              onClick={() => props.onPick(t)}
              className="flex items-center gap-3 rounded-xl px-2 py-1.5 text-left transition-colors enabled:hover:bg-ink/[0.06] enabled:active:bg-ink/[0.1] disabled:opacity-50"
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

