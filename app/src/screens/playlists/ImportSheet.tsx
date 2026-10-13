import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useMemo, useState } from 'react'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import {
  importApply,
  importPreview,
  spotifyPlaylists,
  spotifyRead,
  type ImportChoice,
  type ImportDone,
  type PlaylistItem,
  type PreviewItem,
  type PreviewPlaylist,
  type SpotifyPlaylist,
} from '../../lib/engine'
import { SpotifyAccount, useSpotifyStatus } from '../../components/SpotifyAccount'
import { clock } from '../../lib/player'
import { Detail, Segmented, Switch } from '../../components/ui'
import { playlistsChanged } from '../../components/AddToPlaylist'
import { plural } from './common'

type SourceId = 'spotify' | 'apple' | 'youtube' | 'deezer' | 'others' | 'player'
const ALL = ['json', 'csv', 'xml', 'txt', 'm3u', 'm3u8', 'pls']

const SOURCES: { id: SourceId; name: string; need: string; steps: React.ReactNode[]; accept?: string[] }[] = [
  {
    id: 'spotify',
    name: 'Spotify',
    need: 'Sign in, or an export file',
    accept: ['json', 'csv'],
    steps: [
      <>
        Quickest: open <b>exportify.app</b>, sign in with Spotify and export the playlists you want. Each comes as a .csv file with every song's
        ISRC, so matches are exact.
      </>,
      <>
        Or from Spotify itself: Account, Privacy settings, <b>Download your data</b>, Account data. A few days later you get a zip; choose{' '}
        <b>Playlist1.json</b> from it, and <b>YourLibrary.json</b> for your liked songs.
      </>,
    ],
  },
  {
    id: 'apple',
    name: 'Apple Music',
    need: 'The library on this computer, or an export',
    accept: ['xml', 'txt'],
    steps: [
      <>If Apple Music or iTunes is on this computer, Venuta can read every playlist in it directly. It needs your library key, entered once in Settings, Online sources.</>,
      <>
        Or export a playlist: select it in iTunes or Apple Music, then File, Library, <b>Export Playlist…</b>, as XML or text. Export Library gives
        you all of them at once.
      </>,
    ],
  },
  {
    id: 'youtube',
    name: 'YouTube Music',
    need: 'A Google Takeout export',
    accept: ['csv'],
    steps: [
      <>
        Go to <b>takeout.google.com</b>, deselect all, then choose <b>YouTube and YouTube Music</b> with only “playlists” included. Unzip what
        arrives and choose the .csv files in its playlists folder.
      </>,
      <>Takeout lists videos, not songs, so Venuta asks YouTube for each one's title. That needs the internet, and takes a moment for long playlists.</>,
    ],
  },
  {
    id: 'deezer',
    name: 'Deezer',
    need: 'A link to a public playlist',
    steps: [<>In Deezer, open the playlist, choose Share, and copy its link. The playlist has to be public for this to work.</>],
  },
  {
    id: 'others',
    name: 'TIDAL, Amazon Music and others',
    need: 'A CSV from TuneMyMusic or Soundiiz',
    accept: ['csv'],
    steps: [
      <>
        Open <b>tunemymusic.com</b> or <b>soundiiz.com</b>, connect your service, pick the playlists and choose “Export to file” as CSV. One file
        can hold many playlists; each becomes its own here.
      </>,
    ],
  },
  {
    id: 'player',
    name: 'Another player',
    need: '.m3u, .m3u8 or .pls files',
    accept: ['m3u', 'm3u8', 'pls'],
    steps: [<>foobar2000, MusicBee, VLC, Winamp, Poweramp and most players save playlists this way. Songs are matched by their tags when the files are still there, by name when they're not.</>],
  },
]

interface Pick {
  include: boolean
  name: string
  replace: boolean
  /** Close matches you said no to, by position. */
  rejected: Set<number>
}

type Step = { at: 'pick' } | { at: 'reading'; what: string } | { at: 'review' } | { at: 'saving' } | { at: 'done'; result: ImportDone }

export function ImportSheet(props: {
  root: string
  appleReady: boolean
  onBack: () => void
  onOpen: (name: string) => void
  onOpenWanted: () => void
  onChanged: () => void
}) {
  const { root } = props
  const [step, setStep] = useState<Step>({ at: 'pick' })
  const [source, setSource] = useState<SourceId | null>(null)
  const [link, setLink] = useState('')
  const [lists, setLists] = useState<PreviewPlaylist[]>([])
  const [picks, setPicks] = useState<Pick[]>([])
  const [problems, setProblems] = useState<string[]>([])
  const [dropping, setDropping] = useState(false)

  async function read(files: string[], deezer: string | null, apple: boolean) {
    const youtube = source === 'youtube'
    setProblems([])
    setStep({
      at: 'reading',
      what: apple
        ? 'Reading your Apple Music library and matching every song to yours…'
        : deezer
          ? 'Fetching the playlist from Deezer and matching every song…'
          : `Reading ${files.length === 1 ? 'the file' : `${files.length} files`}${youtube ? ', asking YouTube for song names,' : ''} and matching every song to your library…`,
    })
    try {
      const r = await importPreview(root, files, deezer, apple)
      setLists(r.lists)
      setProblems(r.problems)
      setPicks(r.lists.map((l) => ({ include: l.items.length > 0, name: l.name, replace: false, rejected: new Set() })))
      setStep({ at: 'review' })
    } catch (e) {
      setProblems([e instanceof Error ? e.message : String(e)])
      setStep({ at: 'pick' })
    }
  }

  /** Playlists read from your Spotify account, matched like any import. */
  async function readSpotify(chosen: SpotifyPlaylist[]) {
    setProblems([])
    const songs = chosen.reduce((n, p) => n + p.songs, 0)
    setStep({ at: 'reading', what: `Reading ${plural(chosen.length, 'playlist')} (${plural(songs, 'song')}) from Spotify and matching every song to your library…` })
    try {
      const r = await spotifyRead(root, chosen)
      setLists(r.lists)
      setProblems(r.problems)
      setPicks(r.lists.map((l) => ({ include: l.items.length > 0, name: l.name, replace: false, rejected: new Set() })))
      setStep({ at: 'review' })
    } catch (e) {
      setProblems([e instanceof Error ? e.message : String(e)])
      setStep({ at: 'pick' })
    }
  }

  async function choose(accept: string[] | undefined) {
    const picked = await openDialog({
      multiple: true,
      filters: [{ name: 'Playlist exports', extensions: accept ?? ALL }],
    })
    const files = Array.isArray(picked) ? picked : picked ? [picked] : []
    if (files.length) void read(files, null, false)
  }

  // For checking the flow without a file dialog (development only).
  useEffect(() => {
    if (!import.meta.env.DEV) return
    const w = window as unknown as { __venutaImport?: (files: string[], link?: string) => void }
    w.__venutaImport = (files, link) => void read(files, link ?? null, false)
    return () => {
      delete w.__venutaImport
    }
  })

  // Export files dropped anywhere on the window while choosing a source.
  useEffect(() => {
    if (step.at !== 'pick') return
    let off: (() => void) | undefined
    let live = true
    void getCurrentWebview()
      .onDragDropEvent((e) => {
        if (e.payload.type === 'enter' || e.payload.type === 'over') setDropping(true)
        else if (e.payload.type === 'leave') setDropping(false)
        else if (e.payload.type === 'drop') {
          setDropping(false)
          const files = e.payload.paths.filter((p) => ALL.includes(p.split('.').pop()?.toLowerCase() ?? ''))
          if (files.length) void read(files, null, false)
          else setProblems(["Those aren't playlist files. Venuta reads .json, .csv, .xml, .txt, .m3u, .m3u8 and .pls exports."])
        }
      })
      .then((u) => (live ? (off = u) : u()))
    return () => {
      live = false
      off?.()
      setDropping(false)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [step.at])

  const chosen = picks.map((p, i) => ({ p, l: lists[i] })).filter((x) => x.p.include && x.l)
  const totals = useMemo(() => {
    let found = 0
    let wanted = 0
    for (const { p, l } of chosen)
      l.items.forEach((it, i) => {
        if (it.found && (it.grade === 'exact' || !p.rejected.has(i))) found++
        else wanted++
      })
    return { found, wanted }
  }, [chosen])

  async function apply() {
    const choices: ImportChoice[] = chosen.map(({ p, l }) => ({
      name: p.name.trim() || l.name,
      source: l.source,
      replace: l.exists && p.replace,
      items: l.items.map((it, i): PlaylistItem =>
        it.found && (it.grade === 'exact' || !p.rejected.has(i))
          ? { path: it.found.path }
          : {
              want: {
                title: it.foreign.title,
                artist: it.foreign.artist,
                album: it.foreign.album,
                duration_ms: it.foreign.duration_ms ?? 0,
                ...(it.foreign.isrc ? { isrc: it.foreign.isrc } : {}),
              },
            },
      ),
    }))
    setStep({ at: 'saving' })
    try {
      const result = await importApply(root, choices)
      playlistsChanged()
      props.onChanged()
      setStep({ at: 'done', result })
    } catch (e) {
      setProblems([e instanceof Error ? e.message : String(e)])
      setStep({ at: 'review' })
    }
  }

  const setPick = (i: number, p: Partial<Pick>) => setPicks((all) => all.map((x, j) => (j === i ? { ...x, ...p } : x)))
  const restart = () => {
    setLists([])
    setPicks([])
    setProblems([])
    setLink('')
    setStep({ at: 'pick' })
  }

  return (
    <Detail title="Import playlists" onBack={props.onBack}>
      <h1 className="title text-[34px] leading-[1.1]">Import playlists</h1>
      <p className="mt-2 max-w-[68ch] text-[13.5px] leading-relaxed text-ink/60">
        {step.at === 'review'
          ? 'Here is what Venuta found. Check the close matches, then import. Nothing is written until you do.'
          : step.at === 'done'
            ? 'Done. Your playlists are saved beside your music as .m3u8 files.'
            : "Bring your playlists over from a streaming service or another player. Songs you have are matched to your files; the ones you don't keep their place, so the playlist fills in as your library grows."}
      </p>

      {problems.length > 0 && (
        <div role="alert" className="mt-6 max-w-[760px] rounded-2xl bg-[color-mix(in_oklab,var(--color-problem)_14%,transparent)] px-4 py-3 text-[13px] leading-relaxed text-ink/85">
          {problems.map((p, i) => (
            <p key={i} className="whitespace-pre-line">
              {p}
            </p>
          ))}
        </div>
      )}

      <AnimatePresence mode="wait" initial={false}>
        {step.at === 'pick' && (
          <motion.div key="pick" initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0 }} transition={{ duration: 0.18 }}>
            <div role="radiogroup" aria-label="Where from" className="mt-8 grid grid-cols-[repeat(auto-fill,minmax(230px,1fr))] gap-3">
              {SOURCES.map((s) => {
                const on = source === s.id
                return (
                  <button
                    key={s.id}
                    role="radio"
                    aria-checked={on}
                    onClick={() => setSource(on ? null : s.id)}
                    className={`relative rounded-2xl p-4 text-left transition-[background-color,box-shadow] active:scale-[0.99] ${
                      on
                        ? 'bg-[color-mix(in_oklab,var(--accent)_16%,transparent)] shadow-[inset_0_0_0_1.5px_color-mix(in_oklab,var(--accent)_70%,transparent)]'
                        : 'bg-ink/[0.05] shadow-[inset_0_0_0_1px_var(--edge)] hover:bg-ink/[0.08]'
                    }`}
                  >
                    <span className="block text-[15px] font-semibold">{s.name}</span>
                    <span className="mt-0.5 block text-[12.5px] text-ink/55">{s.need}</span>
                  </button>
                )
              })}
            </div>

            <AnimatePresence mode="wait" initial={false}>
              {source && (
                <motion.section
                  key={source}
                  initial={{ opacity: 0, y: -4 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0 }}
                  transition={{ duration: 0.16 }}
                  className="glass mt-4 max-w-[760px] rounded-[20px] p-5"
                >
                  {(() => {
                    const s = SOURCES.find((x) => x.id === source)!
                    return (
                      <>
                        {source === 'spotify' && (
                          <div className="mb-5 border-b border-edge pb-5">
                            <SpotifyAccount compact />
                            <SpotifyPicker onRead={(c) => void readSpotify(c)} />
                            <p className="mt-4 text-[12.5px] font-medium text-ink/55">Or from a file</p>
                          </div>
                        )}
                        <ol className="grid gap-2.5 text-[13.5px] leading-relaxed text-ink/75">
                          {s.steps.map((t, i) => (
                            <li key={i} className="flex gap-3">
                              {s.steps.length > 1 && <span className="mt-[3px] grid h-5 w-5 shrink-0 place-items-center rounded-full bg-ink/[0.08] text-[11px] font-semibold text-ink/60">{i + 1}</span>}
                              <span>{t}</span>
                            </li>
                          ))}
                        </ol>
                        <div className="mt-5 flex flex-wrap items-center gap-2.5">
                          {source === 'deezer' ? (
                            <form
                              className="flex min-w-[280px] flex-1 gap-2"
                              onSubmit={(e) => {
                                e.preventDefault()
                                if (link.trim()) void read([], link.trim(), false)
                              }}
                            >
                              <input
                                autoFocus
                                value={link}
                                onChange={(e) => setLink(e.target.value)}
                                placeholder="https://www.deezer.com/playlist/…"
                                aria-label="Deezer playlist link"
                                className="field min-w-0 flex-1"
                              />
                              <button disabled={!link.trim()} className="btn-primary">
                                Read playlist
                              </button>
                            </form>
                          ) : source === 'apple' ? (
                            <>
                              <button
                                onClick={() => void read([], null, true)}
                                disabled={!props.appleReady}
                                className="btn-primary"
                                title={props.appleReady ? undefined : 'Enter your library key in Settings, Online sources, first'}
                              >
                                Read my Apple Music library
                              </button>
                              <button onClick={() => void choose(s.accept)} className="btn">
                                Choose an export file…
                              </button>
                              {!props.appleReady && <span className="text-[12.5px] text-ink/50">Reading the library needs its key, in Settings, Online sources.</span>}
                            </>
                          ) : (
                            <button onClick={() => void choose(s.accept)} className="btn-primary">
                              Choose {s.accept?.length === 1 ? `.${s.accept[0]} files` : 'files'}…
                            </button>
                          )}
                        </div>
                      </>
                    )
                  })()}
                </motion.section>
              )}
            </AnimatePresence>

            <div
              className={`mt-6 grid max-w-[760px] place-items-center rounded-[20px] border border-dashed px-6 py-7 text-center transition-colors ${
                dropping ? 'border-[var(--accent)] bg-[color-mix(in_oklab,var(--accent)_10%,transparent)]' : 'border-edge'
              }`}
            >
              <p className="text-[13.5px] text-ink/70">{dropping ? 'Drop to read these files' : 'Already have export files? Drop them here, from any service.'}</p>
              {!dropping && (
                <button onClick={() => void choose(undefined)} className="btn-ghost btn-sm mt-2">
                  Choose files…
                </button>
              )}
            </div>
          </motion.div>
        )}

        {(step.at === 'reading' || step.at === 'saving') && (
          <motion.div key="busy" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} className="mt-16 max-w-[520px]" role="status">
            <div className="relative h-1.5 overflow-hidden rounded-full bg-ink/[0.08]">
              <div className="bar-sweep absolute inset-y-0 w-1/3 rounded-full bg-accent" />
            </div>
            <p className="mt-4 text-[14px] text-ink/70">{step.at === 'reading' ? step.what : 'Writing your playlists…'}</p>
          </motion.div>
        )}

        {step.at === 'review' && (
          <motion.div key="review" initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0 }} transition={{ duration: 0.18 }}>
            {lists.length > 1 && (
              <div className="mt-7 flex items-center gap-3 text-[13px] text-ink/60">
                {plural(lists.length, 'playlist')} found.
                <button onClick={() => setPicks((all) => all.map((p) => ({ ...p, include: true })))} className="btn-ghost btn-sm">
                  Include all
                </button>
                <button onClick={() => setPicks((all) => all.map((p) => ({ ...p, include: false })))} className="btn-ghost btn-sm">
                  Include none
                </button>
              </div>
            )}
            <div className="mt-4 grid gap-3">
              {lists.map((l, i) => (
                <ReviewCard key={i} list={l} pick={picks[i]} onChange={(p) => setPick(i, p)} />
              ))}
            </div>
            <div className="sticky bottom-4 z-10 mt-8 flex flex-wrap items-center gap-3 rounded-[20px] bg-[var(--popover)] p-3 pl-5 shadow-[0_18px_40px_-16px_rgb(0_0_0/0.6),inset_0_0_0_1px_var(--edge)] backdrop-blur-xl">
              <p className="min-w-0 flex-1 text-[13px] text-ink/70">
                {chosen.length
                  ? `${plural(chosen.length, 'playlist')}: ${plural(totals.found, 'song')} from your library${totals.wanted ? `, ${totals.wanted} to get` : ''}.`
                  : 'Include a playlist to import it.'}
              </p>
              <button onClick={restart} className="btn">
                Start over
              </button>
              <button onClick={() => void apply()} disabled={!chosen.length} className="btn-primary">
                Import {chosen.length > 1 ? plural(chosen.length, 'playlist') : 'playlist'}
              </button>
            </div>
          </motion.div>
        )}

        {step.at === 'done' && (
          <motion.div key="done" initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0 }} transition={{ duration: 0.2 }} className="mt-8 max-w-[760px]">
            <div className="grid gap-1.5">
              {step.result.written.map((w) => (
                <div key={w.name} className="glass flex items-center gap-4 rounded-2xl px-4 py-3">
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[14.5px] font-medium">{w.name}</span>
                    <span className="block text-[12.5px] text-ink/55">
                      {plural(w.songs, 'song')} from your library{w.wanted ? `, ${w.wanted} waiting until you have them` : ''}
                    </span>
                  </span>
                  <button onClick={() => props.onOpen(w.name)} className="btn btn-sm">
                    Open
                  </button>
                </div>
              ))}
            </div>
            {step.result.wanted > 0 && (
              <p className="mt-5 text-[13.5px] text-ink/65">
                The missing songs come from {plural(step.result.wanted, 'album')}, now on To get, best first.{' '}
                <button onClick={props.onOpenWanted} className="font-medium text-accent-ink underline-offset-4 hover:underline">
                  See To get
                </button>
              </p>
            )}
            <div className="mt-7 flex gap-2.5">
              <button onClick={props.onBack} className="btn-primary">
                Done
              </button>
              <button onClick={restart} className="btn">
                Import more
              </button>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </Detail>
  )
}

/** Your Spotify playlists to choose from, once you are signed in. */
function SpotifyPicker(props: { onRead: (chosen: SpotifyPlaylist[]) => void }) {
  const [status] = useSpotifyStatus()
  const [lists, setLists] = useState<SpotifyPlaylist[] | null>(null)
  const [chosen, setChosen] = useState<Set<string>>(new Set())
  const [problem, setProblem] = useState('')
  const connected = !!status?.connected
  useEffect(() => {
    if (!connected) {
      setLists(null)
      return
    }
    let live = true
    setProblem('')
    spotifyPlaylists()
      .then((l) => {
        if (!live) return
        setLists(l)
        setChosen(new Set(l.filter((p) => p.readable).map((p) => p.id)))
      })
      .catch((e) => live && setProblem(e instanceof Error ? e.message : String(e)))
    return () => {
      live = false
    }
  }, [connected])
  if (!connected) return null
  if (problem) return <p className="mt-3 text-[12.5px] text-problem">{problem}</p>
  if (!lists)
    return (
      <div className="mt-4 grid gap-1.5" aria-hidden>
        {[0, 1, 2].map((i) => (
          <div key={i} className="shimmer h-10 rounded-xl" />
        ))}
      </div>
    )
  const readable = lists.filter((p) => p.readable)
  const picked = lists.filter((p) => chosen.has(p.id))
  const toggle = (id: string) =>
    setChosen((c) => {
      const n = new Set(c)
      if (n.has(id)) n.delete(id)
      else n.add(id)
      return n
    })
  return (
    <div className="mt-4">
      <div className="flex items-center gap-3 text-[12.5px] text-ink/60">
        <span className="flex-1">{plural(lists.length, 'playlist')} on Spotify</span>
        <button onClick={() => setChosen(new Set(readable.map((p) => p.id)))} className="btn-ghost btn-sm">
          All
        </button>
        <button onClick={() => setChosen(new Set())} className="btn-ghost btn-sm">
          None
        </button>
      </div>
      <div className="fade-y thin-scroll mt-1 grid max-h-[320px] gap-0.5 overflow-y-auto" role="group" aria-label="Spotify playlists">
        {lists.map((p) => (
          <label
            key={p.id}
            className={`flex items-center gap-3 rounded-xl px-2 py-1.5 ${p.readable ? 'cursor-pointer hover:bg-ink/[0.05]' : 'opacity-55'}`}
            title={p.readable ? undefined : "Spotify shares the songs only of playlists you made or collaborate on. Use an Exportify file for this one."}
          >
            <input type="checkbox" checked={chosen.has(p.id)} disabled={!p.readable} onChange={() => toggle(p.id)} className="h-4 w-4 accent-[var(--accent)]" />
            <span className="grid h-9 w-9 shrink-0 place-items-center overflow-hidden rounded-md bg-ink/[0.08] text-[14px]">
              {p.image ? <img src={p.image} alt="" className="h-full w-full object-cover" /> : p.id === 'liked' ? '♥' : '♪'}
            </span>
            <span className="min-w-0 flex-1">
              <span className="block truncate text-[13.5px]">{p.name}</span>
              <span className="block truncate text-[12px] text-ink/50">
                {p.readable ? plural(p.songs, 'song') : `${p.owner}'s playlist: Spotify doesn't share its songs`}
              </span>
            </span>
          </label>
        ))}
      </div>
      <button onClick={() => props.onRead(picked)} disabled={!picked.length} className="btn-primary mt-3">
        {picked.length ? `Read ${plural(picked.length, 'playlist')}` : 'Choose playlists to read'}
      </button>
    </div>
  )
}

type Show = 'check' | 'found' | 'get'

function ReviewCard(props: { list: PreviewPlaylist; pick: Pick | undefined; onChange: (p: Partial<Pick>) => void }) {
  const { list: l, pick } = props
  const [open, setOpen] = useState(false)
  const [show, setShow] = useState<Show>(l.close ? 'check' : 'found')
  const [more, setMore] = useState(false)
  if (!pick) return null
  const rows = l.items
    .map((it, i) => ({ it, i }))
    .filter(({ it }) => (show === 'check' ? it.grade === 'close' : show === 'found' ? it.grade === 'exact' : !it.found))
  const shown = more ? rows : rows.slice(0, 150)
  const accepted = l.close - [...pick.rejected].filter((i) => l.items[i]?.grade === 'close').length
  const toGet = l.missing + (l.close - accepted)
  const nameClash = l.exists && pick.include

  return (
    <section className={`glass rounded-[20px] transition-opacity ${pick.include ? '' : 'opacity-60'}`} aria-label={l.name}>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2 p-4">
        <Switch on={pick.include} onChange={(include) => props.onChange({ include })} label={`Import ${l.name}`} />
        <input
          value={pick.name}
          onChange={(e) => props.onChange({ name: e.target.value })}
          onBlur={() => !pick.name.trim() && props.onChange({ name: l.name })}
          aria-label="Name for this playlist"
          disabled={!pick.include}
          className="min-w-[180px] flex-1 rounded-lg bg-transparent px-2 py-1 text-[15.5px] font-semibold outline-none transition-colors -mx-2 hover:bg-ink/[0.05] focus:bg-ink/[0.07] focus:ring-1 focus:ring-[color-mix(in_oklab,var(--accent)_60%,transparent)]"
        />
        <span className="flex flex-wrap items-center gap-1.5 text-[12px]">
          <span className="rounded-full bg-ink/[0.07] px-2.5 py-1 text-ink/65">{l.source}</span>
          <span className="rounded-full bg-[color-mix(in_oklab,var(--accent)_18%,transparent)] px-2.5 py-1 font-medium text-accent-ink">{l.exact} found</span>
          {l.close > 0 && <span className="rounded-full bg-ink/[0.07] px-2.5 py-1 text-ink/70">{l.close} close {l.close === 1 ? 'match' : 'matches'}</span>}
          {toGet > 0 && <span className="rounded-full bg-ink/[0.07] px-2.5 py-1 text-ink/55">{toGet} to get</span>}
        </span>
        <button
          onClick={() => setOpen((o) => !o)}
          aria-expanded={open}
          disabled={!l.items.length}
          className="btn-ghost btn-sm"
        >
          {open ? 'Hide songs' : l.close ? 'Check songs' : 'Songs'}
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" className={`transition-transform ${open ? 'rotate-180' : ''}`} aria-hidden>
            <path d="m6 9 6 6 6-6" />
          </svg>
        </button>
      </div>
      {!l.items.length && <p className="px-4 pb-4 text-[12.5px] text-ink/50">This playlist is empty, so there is nothing to import.</p>}
      {nameClash && (
        <div className="flex flex-wrap items-center gap-3 border-t border-edge px-4 py-3 text-[13px] text-ink/70">
          <span className="min-w-0 flex-1">You already have a playlist called “{l.name}”.</span>
          <Segmented
            label="When the name is taken"
            size="sm"
            value={pick.replace ? 'replace' : 'both'}
            onChange={(v) => props.onChange({ replace: v === 'replace' })}
            options={[
              { value: 'both', label: 'Keep both' },
              { value: 'replace', label: 'Replace it' },
            ]}
          />
        </div>
      )}
      <AnimatePresence initial={false}>
        {open && (
          <motion.div initial={{ height: 0, opacity: 0 }} animate={{ height: 'auto', opacity: 1 }} exit={{ height: 0, opacity: 0 }} transition={{ duration: 0.22 }} className="overflow-hidden">
            <div className="border-t border-edge px-4 pb-3 pt-3">
              <Segmented
                label="Which songs"
                tabs
                size="sm"
                value={show}
                onChange={(v) => {
                  setShow(v)
                  setMore(false)
                }}
                options={[
                  { value: 'check', label: `Close matches ${l.close}`, disabled: !l.close },
                  { value: 'found', label: `Found ${l.exact}`, disabled: !l.exact },
                  { value: 'get', label: `Not in your library ${l.missing}`, disabled: !l.missing },
                ]}
              />
              {show === 'check' && l.close > 0 && (
                <p className="mt-2.5 text-[12.5px] text-ink/50">Close matches are used unless you say otherwise: a different version, length or spelling. Say “Not it” and the song waits on To get instead.</p>
              )}
              <div className="fade-y mt-2 grid max-h-[420px] gap-0.5 overflow-y-auto">
                {shown.map(({ it, i }) => (
                  <SongRow key={i} item={it} n={i + 1} rejected={pick.rejected.has(i)} onToggle={() => {
                    const r = new Set(pick.rejected)
                    if (r.has(i)) r.delete(i)
                    else r.add(i)
                    props.onChange({ rejected: r })
                  }} />
                ))}
                {rows.length > shown.length && (
                  <button onClick={() => setMore(true)} className="btn-ghost btn-sm my-2 justify-self-center">
                    Show {rows.length - shown.length} more
                  </button>
                )}
              </div>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </section>
  )
}

function SongRow(props: { item: PreviewItem; n: number; rejected: boolean; onToggle: () => void }) {
  const { foreign: f, found, grade, why } = props.item
  return (
    <div className="grid grid-cols-[28px_minmax(0,1fr)_minmax(0,1fr)_auto] items-center gap-3 rounded-xl px-2 py-1.5 text-[13px] hover:bg-ink/[0.04]">
      <span className="text-right text-[12px] tabular-nums text-ink/40">{props.n}</span>
      <span className="min-w-0">
        <span className="block truncate">{f.title || 'Untitled'}</span>
        <span className="block truncate text-[12px] text-ink/50">
          {[f.artist, f.album].filter(Boolean).join(', ') || 'No artist given'}
          {f.duration_ms ? `, ${clock(f.duration_ms)}` : ''}
        </span>
      </span>
      <span className={`min-w-0 ${props.rejected ? 'opacity-45 line-through decoration-ink/40' : ''}`}>
        {found ? (
          <>
            <span className="block truncate">
              <span className="text-ink/40" aria-hidden>
                →{' '}
              </span>
              {found.title}
            </span>
            <span className="block truncate text-[12px] text-ink/50">
              {found.artist}, {found.album}, {clock(found.duration_ms)}
            </span>
            {grade === 'close' && why && <span className="block truncate text-[12px] text-accent-ink">{why}</span>}
          </>
        ) : (
          <span className="text-[12.5px] text-ink/45">Not in your library; waits on To get</span>
        )}
      </span>
      {grade === 'close' ? (
        <button onClick={props.onToggle} aria-pressed={!props.rejected} className={`btn-sm ${props.rejected ? 'btn' : 'btn-ghost'}`}>
          {props.rejected ? 'Use it' : 'Not it'}
        </button>
      ) : (
        <span />
      )}
    </div>
  )
}
