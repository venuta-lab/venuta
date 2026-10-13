import { AnimatePresence, motion, Reorder, useDragControls } from 'motion/react'
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
  smartSave,
  type CatalogTrack,
  type PlaylistEntry,
  type PlaylistItem,
  type PlaylistSummary,
  type SmartRule,
} from '../../lib/engine'
import { clock, enqueue, play, toggle, usePlayerValue, current } from '../../lib/player'
import { notify } from '../../lib/notify'
import { useEscape } from '../../lib/layers'
import { confirmAsk } from '../../components/Confirm'
import { askAddToPlaylist } from '../../components/AddToPlaylist'
import { Mosaic } from '../../components/Mosaic'
import { Detail } from '../../components/ui'
import { IconMore, IconPause, IconPlay, IconSearch, IconShuffle } from '../../components/Icons'
import { Equalizer, useContextMenu } from '../listen/parts'
import { shuffled } from '../listen/model'
import { fromWhere, length, plural, sourceDate } from './common'
import { describeRule, SmartRules } from './SmartRules'

type Row = PlaylistEntry & { key: string }
let keySeq = 0
const withKeys = (entries: PlaylistEntry[]): Row[] => entries.map((e) => ({ ...e, key: `r${++keySeq}` }))
const itemOf = (r: PlaylistEntry): PlaylistItem =>
  r.missing ? { want: r.want ?? { title: r.title, artist: r.artist, album: r.album, duration_ms: r.duration_ms } } : { path: r.path }

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
  const isNow = nowFrom === name

  const playFrom = (row: Row) => {
    if (row.path === nowPath && isNow) return void toggle()
    const i = have.findIndex((r) => r.key === row.key)
    void play(root, songs, Math.max(0, i), name)
  }

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
          ...(missing ? [['Look for missing songs again', () => void refind()]] : []),
        ]) as [string, () => void][]),
    ...((have.length ? [['Give unrated songs four stars', () => void rateUnrated()]] : []) as [string, () => void][]),
    ['Export as a file…', () => void exportFile()],
    ...((smart ? [] : [['Delete…', () => void remove()]]) as [string, () => void][]),
  ])

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
          <button onClick={() => (isNow ? void toggle() : void play(root, songs, 0, name))} className="btn-primary btn-sm">
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
            {rows === null ? 'Reading…' : plural(have.length, 'song')}
            {have.length > 0 && `, ${length(totalMs)}`}
            {missing > 0 && <span className="text-ink/50">, {missing} not in your library</span>}
          </p>
          {smart && props.rule && <p className="mt-1 max-w-[70ch] text-[12.5px] leading-relaxed text-ink/50">{describeRule(props.rule)}</p>}
          <div className="mt-5 flex flex-wrap items-center gap-2.5">
            <button
              disabled={!songs.length}
              onClick={() => (isNow ? void toggle() : void play(root, songs, 0, name))}
              className="btn-primary btn-lg"
            >
              {isNow && playing ? <IconPause /> : <IconPlay />}
              {isNow ? (playing ? 'Pause' : 'Resume') : 'Play'}
            </button>
            <button disabled={songs.length < 2} onClick={() => void play(root, shuffled(songs), 0, name)} className="btn btn-lg">
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

      {rows && rows.length > 0 && (
        <Reorder.Group
          as="div"
          axis="y"
          values={rows}
          onReorder={(v: Row[]) => setRows(v)}
          className="mt-9 grid"
          role="list"
        >
          {rows.map((r, i) => (
            <PlaylistRow
              key={r.key}
              row={r}
              index={i}
              smart={smart}
              state={r.path && r.path === nowPath && isNow ? (playing ? 'playing' : 'paused') : 'no'}
              onPlay={() => playFrom(r)}
              onDropped={() => orderOf(latest.current) !== saved.current && void persist(latest.current)}
              onRemove={() => {
                const before = latest.current
                void persist(
                  before.filter((x) => x.key !== r.key),
                  `Removed “${r.title}” from ${name}.`,
                  before,
                )
              }}
              onFind={() => setFinding(r)}
              onAlbum={() => r.path && props.onAlbum(r.path.slice(0, r.path.lastIndexOf('/')))}
              onAddTo={() => r.path && askAddToPlaylist([r.path], r.title)}
              root={root}
            />
          ))}
        </Reorder.Group>
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
                  ? { ...x, path: t.path, title: t.title, artist: t.artists.join(', '), album: t.album, duration_ms: t.duration_ms, missing: false, want: undefined }
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
  index: number
  smart: boolean
  state: 'no' | 'playing' | 'paused'
  onPlay: () => void
  onDropped: () => void
  onRemove: () => void
  onFind: () => void
  onAlbum: () => void
  onAddTo: () => void
}) {
  const { row: r, state } = props
  const drag = useDragControls()
  // A drag ends with the button let go over the row: that is not a click to play.
  const dragged = useRef(false)
  const cover = useCovers(320)(r.path ? r.path.slice(0, r.path.lastIndexOf('/')) : '')
  const isNow = state !== 'no'
  const ctx = useContextMenu(() =>
    r.missing
      ? [['Find it in your library…', props.onFind], ...((props.smart ? [] : [['Remove from playlist', props.onRemove]]) as [string, () => void][])]
      : [
          ['Play', props.onPlay],
          ['Play next', () => void enqueue(props.root, tracksFor([r.path]), true)],
          ['Add to queue', () => void enqueue(props.root, tracksFor([r.path]), false)],
          ['Add to another playlist…', props.onAddTo],
          ['Go to album', props.onAlbum],
          ...((props.smart ? [] : [['Remove from playlist', props.onRemove]]) as [string, () => void][]),
        ],
  )
  return (
    <Reorder.Item
      as="div"
      value={r}
      dragListener={false}
      dragControls={drag}
      onDragStart={() => (dragged.current = true)}
      onDragEnd={() => {
        props.onDropped()
        window.setTimeout(() => (dragged.current = false), 0)
      }}
      role="listitem"
      tabIndex={0}
      onContextMenu={ctx.onContextMenu}
      onClick={(e) => {
        if (dragged.current || (e.target as HTMLElement).closest('button')) return
        if (!r.missing) props.onPlay()
      }}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return
        if (e.key === 'Enter' && !r.missing) props.onPlay()
        if (e.key === 'Delete' && !props.smart) props.onRemove()
      }}
      whileDrag={{ scale: 1.01, boxShadow: '0 14px 32px -12px rgb(0 0 0 / 0.55)', backgroundColor: 'var(--popover)' }}
      className={`group relative grid min-h-[56px] grid-cols-[22px_32px_44px_minmax(0,1fr)_minmax(0,0.8fr)_52px_32px] items-center gap-3 rounded-xl px-2 py-1.5 transition-colors ${
        r.missing ? 'cursor-default' : 'cursor-pointer hover:bg-ink/[0.05] active:bg-ink/[0.09]'
      } ${isNow ? 'text-accent-ink' : ''}`}
    >
      {ctx.menu}
      {props.smart ? (
        <span />
      ) : (
        <span
          onPointerDown={(e) => drag.start(e)}
          className="grid h-8 cursor-grab touch-none place-items-center text-ink/25 opacity-0 transition-opacity hover:text-ink/70 active:cursor-grabbing group-hover:opacity-100"
          title="Drag to move"
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
      <span className={`h-11 w-11 overflow-hidden rounded-lg bg-ink/[0.07] ${r.missing ? 'opacity-50' : ''}`}>
        {cover ? <img src={cover.src} alt="" className="h-full w-full object-cover" draggable={false} /> : <span className="grid h-full w-full place-items-center text-ink/30">♪</span>}
      </span>
      <span className={`min-w-0 ${r.missing ? 'opacity-60' : ''}`}>
        <span className="block truncate text-[14px] font-medium">{r.title}</span>
        <span className={`block truncate text-[12.5px] ${isNow ? 'opacity-80' : 'text-ink/55'}`}>{r.artist || '—'}</span>
      </span>
      <span className="min-w-0 truncate text-[13px] text-ink/55">
        {r.missing ? (
          <span className="flex min-w-0 items-center gap-2">
            <span className="truncate text-ink/40">Not in your library</span>
            <button onClick={props.onFind} className="btn-ghost btn-sm shrink-0" title="Pick the song in your library">
              Find
            </button>
          </span>
        ) : (
          r.album
        )}
      </span>
      <span className="text-right text-[12.5px] tabular-nums text-ink/55">{r.duration_ms ? clock(r.duration_ms) : ''}</span>
      {props.smart ? (
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
    </Reorder.Item>
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

