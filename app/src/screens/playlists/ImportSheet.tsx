import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { invoke } from '@tauri-apps/api/core'
import {
  importApply,
  importPreview,
  playlistRestore,
  playlistsCheck,
  playlistUpdateRead,
  spotifyPlaylists,
  spotifyRead,
  type ImportChoice,
  type ImportDone,
  type ImportRead,
  type ImportWritten,
  type PlaylistItem,
  type PlaylistSummary,
  type PreviewItem,
  type PreviewPlaylist,
  type SpotifyPlaylist,
  type UpdateTarget,
} from '../../lib/engine'
import { BOOKMARK_URL } from '../../lib/bookmarklet'
import { SendOut } from './SendOut'
import { SpotifyAccount, useSpotifyStatus } from '../../components/SpotifyAccount'
import { clock } from '../../lib/player'
import { Detail, Segmented, Switch } from '../../components/ui'
import { playlistsChanged } from '../../components/AddToPlaylist'
import { notify } from '../../lib/notify'
import { plural } from './common'

type SourceId = 'spotify' | 'apple' | 'youtube' | 'deezer' | 'jiosaavn' | 'tidal' | 'others' | 'player' | 'backup'
// What Venuta reads (by what's inside; the name is only a hint). A folder (no extension) is read whole.
const ALL = ['json', 'jspf', 'csv', 'xlsx', 'xml', 'txt', 'm3u', 'm3u8', 'pls', 'xspf', 'wpl', 'zpl', 'asx', 'zip']

/** One way to bring a service's playlists in, and what to press for it. */
interface Way {
  title: string
  /** What arrives, in a few words ("Any playlist or Liked Songs, any length"). */
  line: string
  steps?: React.ReactNode[]
  /** paste: what you copied (a link, songs, a list); files: choose files; spotify / apple: their own panel; bookmark: the Copy for Venuta bookmark. */
  action: 'paste' | 'files' | 'spotify' | 'apple' | 'bookmark'
  accept?: string[]
}

const BOOKMARK_WAY = (site: string, what: string): Way => ({
  title: 'The Copy for Venuta bookmark',
  line: `${what}, on ${site} in your browser, signed in`,
  action: 'bookmark',
})

const SOURCES: { id: SourceId; name: string; need: string; ways: Way[] }[] = [
  {
    id: 'spotify',
    name: 'Spotify',
    need: 'Copy from the app, a link, or an export',
    ways: [
      {
        title: "Copy from Spotify's app",
        line: 'Any playlist or Liked Songs, any length',
        steps: [
          <>In Spotify's desktop app, open the playlist, or Liked Songs.</>,
          <>
            Click a song, press <b>Ctrl+A</b> to select them all, then <b>Ctrl+C</b>.
          </>,
          <>Come back here and paste.</>,
        ],
        action: 'paste',
      },
      {
        title: 'A share link',
        line: 'Public playlists, the first 100 songs',
        steps: [<>In Spotify, the playlist's menu, Share, Copy link to playlist. Then paste it here.</>],
        action: 'paste',
      },
      {
        title: 'An export file',
        line: 'Exportify (with recording codes), or Spotify’s own data download',
        steps: [
          <>
            <b>exportify.app</b>: sign in with Spotify and export the playlists you want, as CSV files.
          </>,
          <>
            Or Spotify's Account, Privacy settings, <b>Download your data</b>: a few days later a zip arrives; choose the zip itself.
          </>,
        ],
        action: 'files',
      },
      { title: 'Your Spotify account', line: 'Needs Spotify Premium and your own free developer app', action: 'spotify' },
    ],
  },
  {
    id: 'apple',
    name: 'Apple Music',
    need: 'A link, the bookmark, or this computer’s library',
    ways: [
      {
        title: 'A share link',
        line: 'A playlist you made public',
        steps: [<>In Apple Music, the playlist's menu, Share, Copy Link. Then paste it here.</>],
        action: 'paste',
      },
      BOOKMARK_WAY('music.apple.com', 'Your playlists and Favourite Songs'),
      { title: 'The library on this computer', line: 'Every playlist in Apple Music or iTunes here', action: 'apple' },
      {
        title: 'An export file',
        line: 'From iTunes, or Music on a Mac',
        steps: [<>Select the playlist, then File, Library, <b>Export Playlist…</b> (XML or text). Export Library gives you all of them.</>],
        action: 'files',
      },
    ],
  },
  {
    id: 'youtube',
    name: 'YouTube Music',
    need: 'A link, the bookmark, or Google Takeout',
    ways: [
      {
        title: 'A share link',
        line: 'Public or unlisted playlists, any length',
        steps: [<>On the playlist, Share, Copy link. Then paste it here.</>],
        action: 'paste',
      },
      BOOKMARK_WAY('music.youtube.com', 'Liked Music and your playlists'),
      {
        title: 'Google Takeout',
        line: 'Every playlist, Liked Music too; takes a while to arrive',
        steps: [
          <>
            <b>takeout.google.com</b>: deselect all, choose <b>YouTube and YouTube Music</b> with only playlists.
          </>,
          <>Choose the zip that arrives. Venuta asks YouTube for each song's name.</>,
        ],
        action: 'files',
      },
    ],
  },
  {
    id: 'jiosaavn',
    name: 'JioSaavn',
    need: 'A link, or the bookmark',
    ways: [
      { title: 'A share link', line: 'A public playlist', steps: [<>On the playlist, Share, Copy link. Then paste it here.</>], action: 'paste' },
      BOOKMARK_WAY('jiosaavn.com', 'Your playlists and liked songs'),
    ],
  },
  {
    id: 'deezer',
    name: 'Deezer',
    need: 'A link to a playlist or your profile',
    ways: [
      {
        title: 'A share link',
        line: 'A public playlist, or a public profile’s favourites',
        steps: [<>In Deezer, Share, Copy link (a playlist, or your profile for your favourites). Then paste it here.</>],
        action: 'paste',
      },
    ],
  },
  {
    id: 'tidal',
    name: 'TIDAL',
    need: 'A link, or the bookmark',
    ways: [
      { title: 'A share link', line: 'A public playlist', steps: [<>In TIDAL, Share, Copy playlist link. Then paste it here.</>], action: 'paste' },
      BOOKMARK_WAY('listen.tidal.com', 'Your playlists and favourites'),
    ],
  },
  {
    id: 'others',
    name: 'Amazon Music and others',
    need: 'The bookmark, or a transfer tool’s file',
    ways: [
      BOOKMARK_WAY('their web player', 'Any playlist the page shows'),
      {
        title: 'A file from TuneMyMusic or Soundiiz',
        line: 'Any service they connect to',
        steps: [<>Open <b>tunemymusic.com</b> or <b>soundiiz.com</b>, connect your service, pick the playlists and export them as a CSV file.</>],
        action: 'files',
      },
    ],
  },
  {
    id: 'player',
    name: 'Another player or a phone',
    need: 'Playlist files, or a whole folder',
    ways: [
      {
        title: 'Playlist files',
        line: 'M3U, M3U8, PLS, XSPF, WPL, ASX, from any player or phone',
        steps: [<>foobar2000, MusicBee, VLC, Winamp, Poweramp and most players save playlists this way. Songs from another computer or a phone find the same files here.</>],
        action: 'files',
      },
    ],
  },
  {
    id: 'backup',
    name: 'A Venuta backup',
    need: 'Put your playlists back',
    ways: [{ title: 'The backup file', line: 'Every playlist as it was, empty ones too', action: 'files', accept: ['zip'] }],
  },
]

/** A short name for what's in the box, before it's read ("120 songs copied from Spotify"). */
function whatsPasted(text: string): string {
  const t = text.trim()
  if (!t) return ''
  const lines = t.split(/\r?\n/).map((l) => l.trim()).filter(Boolean)
  const head = lines[0].match(/^#VENUTA-PLAYLIST:(.*?)\|(.*)$/)
  if (head) return `“${head[1].trim() || 'Copied songs'}” from ${head[2].trim()}, ${plural(lines.length - 1, 'song')}`
  const spotifySongs = lines.filter((l) => /open\.spotify\.com\/(?:intl-[a-z-]+\/)?track\/|spotify:track:/.test(l)).length
  if (spotifySongs) return `${plural(spotifySongs, 'song')} copied from Spotify`
  const links = lines.filter((l) => /^https?:\/\//.test(l))
  if (links.length === lines.length) {
    const site = (l: string) =>
      /spotify/.test(l) ? 'Spotify' : /music\.apple/.test(l) ? 'Apple Music' : /youtu/.test(l) ? 'YouTube Music' : /tidal/.test(l) ? 'TIDAL' : /jiosaavn/.test(l) ? 'JioSaavn' : /deezer/.test(l) ? 'Deezer' : /listenbrainz/.test(l) ? 'ListenBrainz' : 'a website'
    return links.length === 1 ? `A link to ${site(links[0])}` : `${links.length} links`
  }
  return `A list of ${plural(lines.length, 'song')}`
}

/** What an incoming playlist does to one you have: fills it in, sits beside it, or replaces it. */
type Mode = 'update' | 'both' | 'replace'

interface Pick {
  include: boolean
  name: string
  mode: Mode
  /** Close matches you said no to, by position. */
  rejected: Set<number>
}

type Step = { at: 'pick' } | { at: 'reading'; what: string } | { at: 'review' } | { at: 'saving' } | { at: 'done'; result: ImportDone }

/** What the source said about a song, kept with it (so a later update knows the song). */
const wantOf = (it: PreviewItem) => ({
  title: it.foreign.title,
  artist: it.foreign.artist,
  album: it.foreign.album,
  duration_ms: it.foreign.duration_ms ?? 0,
  ...(it.foreign.isrc ? { isrc: it.foreign.isrc } : {}),
})

/** What updating a playlist does, in words. */
function updateLine(t: UpdateTarget, source: string) {
  if (!t.added && !t.removed) return `It already has every song ${source} has; nothing changes.`
  const parts = [
    t.added ? `${plural(t.added, 'song')} added from ${source}` : '',
    t.yours ? `${t.yours} you added ${t.yours === 1 ? 'stays' : 'stay'}` : '',
    t.removed ? `${t.removed} ${source} dropped ${t.removed === 1 ? 'goes' : 'go'}` : '',
    t.skipped ? `${t.skipped} you took out ${t.skipped === 1 ? 'stays' : 'stay'} out` : '',
  ].filter(Boolean)
  return `${t.songs} → ${plural(t.after, 'song')}: ${parts.join(', ')}. ${t.after_here} of them in your library, the rest wait in their places.`
}

/** Opened to do one thing at once: complete playlists from Apple Music, update one from its
 *  source, check every playlist's source for changes, or send playlists out. */
export type ImportStart =
  | { kind: 'apple'; only: string[] }
  | { kind: 'update'; name: string }
  | { kind: 'check' }
  | { kind: 'send'; names: string[] }
  /** A playlist link pasted in Open link: read it at once. */
  | { kind: 'link'; link: string }

const pickFor = (l: PreviewPlaylist, include = l.items.length > 0 || l.source === 'Venuta backup'): Pick => ({
  include,
  name: l.target?.name ?? l.name,
  mode: l.target ? 'update' : 'both',
  rejected: new Set(),
})

type Side = 'in' | 'out'

export function ImportSheet(props: {
  root: string
  appleReady: boolean
  /** Your playlists, to send out. */
  playlists: PlaylistSummary[]
  start?: ImportStart
  onBack: () => void
  onOpen: (name: string) => void
  onOpenWanted: () => void
  onChanged: () => void
}) {
  const { root } = props
  const [side, setSide] = useState<Side>(props.start?.kind === 'send' ? 'out' : 'in')
  const [step, setStep] = useState<Step>({ at: 'pick' })
  const [source, setSource] = useState<SourceId | null>(null)
  const [text, setText] = useState('')
  const [progress, setProgress] = useState<{ done: number; total: number; note: string } | null>(null)
  const [lists, setLists] = useState<PreviewPlaylist[]>([])
  const [picks, setPicks] = useState<Pick[]>([])
  const [problems, setProblems] = useState<string[]>([])
  const [dropping, setDropping] = useState(false)
  const [upToDate, setUpToDate] = useState(false)
  const box = useRef<HTMLTextAreaElement>(null)
  const shownFor = useRef<SourceId | null>(null)
  const onProgress = (done: number, total: number, note?: string) => setProgress({ done, total, note: note ?? '' })

  /** Read, then show the review: `only` keeps just those playlists (by the source's names). */
  async function review(what: string, get: () => Promise<ImportRead>, only?: string[]) {
    setProblems([])
    setUpToDate(false)
    setProgress(null)
    setStep({ at: 'reading', what })
    try {
      const r = await get()
      const lists = only ? r.lists.filter((l) => only.includes(l.name)) : r.lists
      setLists(lists)
      setProblems(r.problems)
      setPicks(lists.map((l) => pickFor(l)))
      setStep({ at: 'review' })
      return lists.length
    } catch (e) {
      setProblems([e instanceof Error ? e.message : String(e)])
      setStep({ at: 'pick' })
      return -1
    } finally {
      setProgress(null)
    }
  }

  function read(files: string[], link: string | null, apple: boolean, only?: string[], pasted: string | null = null) {
    return review(
      apple
        ? 'Reading your Apple Music library and matching every song to yours…'
        : pasted !== null
          ? `Reading ${whatsPasted(pasted).replace(/^A /, 'a ') || 'what you pasted'} and matching every song to your library…`
          : link
            ? 'Fetching the playlist and matching every song…'
            : `Reading ${files.length === 1 ? 'the file' : `${files.length} files`} and matching every song to your library…`,
      () => importPreview(root, files, link, apple, pasted, onProgress),
      only,
    )
  }

  /** What's in the box, brought in. */
  function bringText(t = text) {
    if (t.trim()) void read([], null, false, undefined, t)
  }

  /** What you copied (a link, songs, a list), brought in at once. */
  async function pasteCopied() {
    try {
      const t = await invoke<string>('clipboard_text')
      if (!t.trim()) {
        setProblems(['Nothing is copied yet. Copy a link, songs or a list first.'])
        return
      }
      setText(t)
      bringText(t)
    } catch (e) {
      setProblems([e instanceof Error ? e.message : String(e)])
    }
  }

  /** Every playlist that came from somewhere readable, read again. */
  async function checkAll() {
    const n = await review('Checking where your playlists came from for new songs…', () => playlistsCheck(root, onProgress))
    if (n === 0) {
      setStep({ at: 'pick' })
      setUpToDate(true)
    }
  }

  // Opened to do one thing: start on it at once.
  const started = useRef(false)
  useEffect(() => {
    const s = props.start
    if (!s || started.current) return
    started.current = true
    if (s.kind === 'apple') {
      setSource('apple')
      void read([], null, true, s.only)
    } else if (s.kind === 'update') void review(`Reading where “${s.name}” came from and matching every song…`, () => playlistUpdateRead(root, s.name))
    else if (s.kind === 'check') void checkAll()
    else if (s.kind === 'link') void read([], s.link, false)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.start])

  /** Playlists read from your Spotify account, matched like any import. */
  async function readSpotify(chosen: SpotifyPlaylist[]) {
    setProblems([])
    const songs = chosen.reduce((n, p) => n + p.songs, 0)
    setStep({ at: 'reading', what: `Reading ${plural(chosen.length, 'playlist')} (${plural(songs, 'song')}) from Spotify and matching every song to your library…` })
    try {
      const r = await spotifyRead(root, chosen)
      setLists(r.lists)
      setProblems(r.problems)
      setPicks(r.lists.map((l) => pickFor(l)))
      setStep({ at: 'review' })
    } catch (e) {
      setProblems([e instanceof Error ? e.message : String(e)])
      setStep({ at: 'pick' })
    }
  }

  async function choose(accept: string[] | undefined, folder = false) {
    const picked = await openDialog(
      folder ? { directory: true, title: 'Choose a folder of playlists' } : { multiple: true, filters: [{ name: 'Playlists', extensions: accept ?? ALL }] },
    )
    const files = Array.isArray(picked) ? picked : picked ? [picked] : []
    if (files.length) void read(files, null, false)
  }

  // Ctrl+V anywhere on Bring in (outside another field) puts what you copied in the box.
  useEffect(() => {
    if (side !== 'in' || step.at !== 'pick') return
    const onPaste = (e: ClipboardEvent) => {
      const t = e.target as HTMLElement | null
      if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return
      const pasted = e.clipboardData?.getData('text') ?? ''
      if (!pasted.trim()) return
      e.preventDefault()
      setText(pasted)
      box.current?.focus()
    }
    window.addEventListener('paste', onPaste)
    return () => window.removeEventListener('paste', onPaste)
  }, [side, step.at])

  // For checking the flow without a file dialog (development only).
  useEffect(() => {
    if (!import.meta.env.DEV) return
    const w = window as unknown as { __venutaImport?: (files: string[], link?: string) => void }
    w.__venutaImport = (files, link) => void read(files, link ?? null, false)
    return () => {
      delete w.__venutaImport
    }
  })

  // Playlist files or folders dropped anywhere on the window while bringing in.
  useEffect(() => {
    if (step.at !== 'pick' || side !== 'in') return
    let off: (() => void) | undefined
    let live = true
    void getCurrentWebview()
      .onDragDropEvent((e) => {
        if (e.payload.type === 'enter' || e.payload.type === 'over') setDropping(true)
        else if (e.payload.type === 'leave') setDropping(false)
        else if (e.payload.type === 'drop') {
          setDropping(false)
          const files = e.payload.paths.filter((p) => {
            const name = p.split(/[\\/]/).pop() ?? ''
            return !name.includes('.') || ALL.includes(name.split('.').pop()?.toLowerCase() ?? '')
          })
          if (files.length) void read(files, null, false)
          else setProblems(["Those aren't playlists. Venuta reads playlist files (M3U, PLS, XSPF, JSPF, WPL, ASX), CSV and Excel exports, data downloads and backups (zips), and folders of them."])
        }
      })
      .then((u) => (live ? (off = u) : u()))
    return () => {
      live = false
      off?.()
      setDropping(false)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [step.at, side])

  const chosen = picks.map((p, i) => ({ p, l: lists[i] })).filter((x) => x.p.include && x.l)
  const totals = useMemo(() => {
    let found = 0
    let online = 0
    let wanted = 0
    for (const { p, l } of chosen)
      l.items.forEach((it, i) => {
        if (it.found && (it.grade === 'exact' || !p.rejected.has(i))) found++
        else if (it.online) online++
        else wanted++
      })
    return { found, online, wanted }
  }, [chosen])

  async function apply() {
    const choices: ImportChoice[] = chosen.map(({ p, l }) => ({
      name: p.mode === 'update' && l.target ? l.target.name : p.name.trim() || l.name,
      source: l.source,
      source_id: l.source_id,
      update: p.mode === 'update' && l.target ? l.target.name : null,
      replace: l.exists && p.mode === 'replace',
      items: l.items.map((it, i): PlaylistItem =>
        it.found && (it.grade === 'exact' || !p.rejected.has(i))
          ? { path: it.found.path, source: wantOf(it) }
          : it.online
            ? { online: it.online }
            : // "Not it": that file is remembered, so it's never offered or filled in again.
              { want: it.found ? { ...wantOf(it), not: [it.found.path] } : wantOf(it) },
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

  async function undo(w: ImportWritten) {
    if (!w.copy) return
    try {
      await playlistRestore(root, w.name, w.copy)
      playlistsChanged()
      props.onChanged()
      setUndone((u) => new Set(u).add(w.name))
      notify(`“${w.name}” is back as it was.`)
    } catch (e) {
      notify(`Couldn't put “${w.name}” back: ${e instanceof Error ? e.message : String(e)}`)
    }
  }
  const [undone, setUndone] = useState<Set<string>>(new Set())

  const setPick = (i: number, p: Partial<Pick>) => setPicks((all) => all.map((x, j) => (j === i ? { ...x, ...p } : x)))
  const restart = () => {
    setUndone(new Set())
    setLists([])
    setPicks([])
    setProblems([])
    setText('')
    setStep({ at: 'pick' })
  }

  return (
    <Detail title="Import & export" onBack={props.onBack}>
      <h1 className="title text-[34px] leading-[1.1]">Import & export</h1>
      {step.at === 'pick' && (
        <Segmented
          tabs
          label="Import or export"
          className="mt-5"
          value={side}
          onChange={(v) => {
            setSide(v)
            setProblems([])
          }}
          options={[
            { value: 'in', label: 'Bring in' },
            { value: 'out', label: 'Send out' },
          ]}
        />
      )}
      <p className="mt-4 max-w-[68ch] text-[13.5px] leading-relaxed text-ink/60">
        {side === 'out' && step.at === 'pick'
          ? 'Send playlists to a file for another player, phone or app, to a backup, or to a streaming service, free. Songs you don’t have keep their places in the files that can hold them.'
          : step.at === 'review'
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

      {side === 'out' && step.at === 'pick' ? (
        <SendOut root={root} playlists={props.playlists} start={props.start?.kind === 'send' ? { names: props.start.names } : undefined} />
      ) : (
      <AnimatePresence mode="wait" initial={false}>
        {step.at === 'pick' && (
          <motion.div key="pick" initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0 }} transition={{ duration: 0.18 }} className="max-w-[860px]">
            {/* One box for anything: a link, songs copied from an app, a list; files and folders dropped. */}
            <section
              aria-label="Bring in"
              className={`mt-7 rounded-[22px] p-4 transition-[background-color,box-shadow] ${
                dropping
                  ? 'bg-[color-mix(in_oklab,var(--accent)_12%,transparent)] shadow-[inset_0_0_0_1.5px_color-mix(in_oklab,var(--accent)_70%,transparent)]'
                  : 'glass'
              }`}
            >
              <textarea
                ref={box}
                value={text}
                onChange={(e) => setText(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && !e.shiftKey && text.trim()) {
                    e.preventDefault()
                    bringText()
                  }
                }}
                rows={text.includes('\n') ? 4 : 2}
                placeholder={dropping ? 'Drop to read these files' : 'Paste a playlist link, songs copied from Spotify’s app, or a list of songs. Or drop files or a folder.'}
                aria-label="A link, copied songs, or a list of songs"
                className="w-full resize-none rounded-xl bg-transparent px-2 py-1.5 text-[14px] leading-relaxed outline-none placeholder:text-ink/45"
              />
              <div className="mt-2 flex flex-wrap items-center gap-2.5 border-t border-edge pt-3">
                {text.trim() ? (
                  <>
                    <span className="min-w-0 flex-1 truncate text-[12.5px] text-ink/60">{whatsPasted(text)}</span>
                    <button onClick={() => setText('')} className="btn-ghost btn-sm">
                      Clear
                    </button>
                    <button onClick={() => bringText()} className="btn-primary">
                      Bring in
                    </button>
                  </>
                ) : (
                  <>
                    <button onClick={() => void pasteCopied()} className="btn-primary btn-sm">
                      Paste what you copied
                    </button>
                    <button onClick={() => void choose(undefined)} className="btn btn-sm">
                      Choose files…
                    </button>
                    <button onClick={() => void choose(undefined, true)} className="btn btn-sm">
                      Choose a folder…
                    </button>
                  </>
                )}
              </div>
            </section>

            <h2 className="mt-9 text-[15px] font-semibold">Where are your playlists?</h2>
            <div role="radiogroup" aria-label="Where from" className="mt-3 grid grid-cols-[repeat(auto-fill,minmax(200px,1fr))] gap-2.5">
              {SOURCES.map((s) => {
                const on = source === s.id
                return (
                  <button
                    key={s.id}
                    role="radio"
                    aria-checked={on}
                    onClick={() => setSource(on ? null : s.id)}
                    className={`relative rounded-2xl p-3.5 text-left transition-[background-color,box-shadow] active:scale-[0.99] ${
                      on
                        ? 'bg-[color-mix(in_oklab,var(--accent)_16%,transparent)] shadow-[inset_0_0_0_1.5px_color-mix(in_oklab,var(--accent)_70%,transparent)]'
                        : 'bg-ink/[0.05] shadow-[inset_0_0_0_1px_var(--edge)] hover:bg-ink/[0.08]'
                    }`}
                  >
                    <span className="block text-[14.5px] font-semibold">{s.name}</span>
                    <span className="mt-0.5 block text-[12px] leading-snug text-ink/55">{s.need}</span>
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
                  ref={(el) => {
                    // Brought into view once each time a service is chosen, not on every keystroke.
                    if (el && shownFor.current !== source) {
                      shownFor.current = source
                      el.scrollIntoView({ behavior: 'smooth', block: 'nearest' })
                    }
                  }}
                  className="glass mt-4 scroll-mb-28 rounded-[20px] p-2"
                  aria-label={`Ways in from ${SOURCES.find((x) => x.id === source)!.name}`}
                >
                  {SOURCES.find((x) => x.id === source)!.ways.map((w, i) => (
                    <div key={w.title} className={`p-4 ${i ? 'border-t border-edge' : ''}`}>
                      <div className="flex flex-wrap items-baseline gap-x-3">
                        <h3 className="text-[14.5px] font-semibold">{w.title}</h3>
                        <span className="text-[12.5px] text-ink/55">{w.line}</span>
                      </div>
                      {w.steps && (
                        <ol className="mt-2.5 grid gap-2 text-[13px] leading-relaxed text-ink/75">
                          {w.steps.map((t, j) => (
                            <li key={j} className="flex gap-3">
                              {w.steps!.length > 1 && <span className="mt-[3px] grid h-5 w-5 shrink-0 place-items-center rounded-full bg-ink/[0.08] text-[11px] font-semibold text-ink/60">{j + 1}</span>}
                              <span>{t}</span>
                            </li>
                          ))}
                        </ol>
                      )}
                      <div className="mt-3 flex flex-wrap items-center gap-2.5">
                        {w.action === 'paste' ? (
                          <button onClick={() => void pasteCopied()} className="btn">
                            Paste what you copied
                          </button>
                        ) : w.action === 'files' ? (
                          <button onClick={() => void choose(w.accept)} className="btn">
                            Choose {w.accept?.length === 1 ? `the .${w.accept[0]} file` : 'files'}…
                          </button>
                        ) : w.action === 'apple' ? (
                          <>
                            <button
                              onClick={() => void read([], null, true)}
                              disabled={!props.appleReady}
                              className="btn"
                              title={props.appleReady ? undefined : 'Enter your library key in Settings, Online sources, first'}
                            >
                              Read my Apple Music library
                            </button>
                            {!props.appleReady && <span className="text-[12.5px] text-ink/50">It needs the library's key, in Settings, Online sources.</span>}
                          </>
                        ) : w.action === 'spotify' ? (
                          <div className="w-full">
                            <SpotifyAccount compact />
                            <SpotifyPicker onRead={(c) => void readSpotify(c)} />
                          </div>
                        ) : (
                          <Bookmark onPaste={() => void pasteCopied()} />
                        )}
                      </div>
                    </div>
                  ))}
                </motion.section>
              )}
            </AnimatePresence>

            <p className="mt-8 text-[13px] text-ink/60">
              {upToDate ? 'Every playlist is up to date with where it came from. ' : 'Brought playlists in before? '}
              <button onClick={() => void checkAll()} className="font-medium text-accent-ink underline-offset-4 hover:underline">
                {upToDate ? 'Check again' : 'Check them for new songs'}
              </button>
            </p>
          </motion.div>
        )}

        {(step.at === 'reading' || step.at === 'saving') && (
          <motion.div key="busy" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} className="mt-16 max-w-[520px]" role="status">
            <div className="relative h-1.5 overflow-hidden rounded-full bg-ink/[0.08]">
              {progress && progress.total > 0 ? (
                <div className="absolute inset-y-0 left-0 rounded-full bg-accent transition-[width]" style={{ width: `${Math.round((progress.done / progress.total) * 100)}%` }} />
              ) : (
                <div className="bar-sweep absolute inset-y-0 w-1/3 rounded-full bg-accent" />
              )}
            </div>
            <p className="mt-4 text-[14px] text-ink/70">{step.at === 'reading' ? step.what : 'Writing your playlists…'}</p>
            {step.at === 'reading' && progress && progress.total > 0 && (
              <p className="mt-1 text-[12.5px] tabular-nums text-ink/50">
                {progress.note || 'Working'}: {progress.done.toLocaleString('en-IN')} of {progress.total.toLocaleString('en-IN')}
              </p>
            )}
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
                  ? `${plural(chosen.length, 'playlist')}: ${plural(totals.found, 'song')} from your library${totals.online ? `, ${totals.online} online` : ''}${totals.wanted ? `, ${totals.wanted} to get` : ''}.`
                  : 'Include a playlist to import it.'}
              </p>
              <button onClick={restart} className="btn">
                Start over
              </button>
              <button onClick={() => void apply()} disabled={!chosen.length} className="btn-primary">
                {(() => {
                  const updates = chosen.filter(({ p, l }) => p.mode === 'update' && l.target).length
                  const fresh = chosen.length - updates
                  if (!updates) return `Import ${fresh > 1 ? plural(fresh, 'playlist') : 'playlist'}`
                  if (!fresh) return `Update ${updates > 1 ? plural(updates, 'playlist') : 'playlist'}`
                  return `Import ${fresh}, update ${updates}`
                })()}
              </button>
            </div>
          </motion.div>
        )}

        {step.at === 'done' && (
          <motion.div key="done" initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0 }} transition={{ duration: 0.2 }} className="mt-8 max-w-[760px]">
            <div className="grid gap-1.5">
              {step.result.written.map((w) => {
                const back = undone.has(w.name)
                return (
                  <div key={w.name} className="glass flex items-center gap-4 rounded-2xl px-4 py-3">
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-[14.5px] font-medium">{w.name}</span>
                      <span className="block text-[12.5px] text-ink/55">
                        {back
                          ? 'Put back as it was before.'
                          : `${w.updated ? `Updated: ${w.added ? `${plural(w.added, 'song')} added` : 'no songs added'}${w.removed ? `, ${w.removed} dropped` : ''}. ` : ''}${plural(w.songs, 'song')} in your library${w.online ? `, ${w.online} online` : ''}${w.wanted ? `, ${w.wanted} waiting until you have them` : ''}.`}
                      </span>
                    </span>
                    {w.updated && w.copy && !back && (
                      <button onClick={() => void undo(w)} className="btn-ghost btn-sm" title="Put this playlist back as it was before the update">
                        Undo
                      </button>
                    )}
                    <button onClick={() => props.onOpen(w.name)} className="btn btn-sm">
                      Open
                    </button>
                  </div>
                )
              })}
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
      )}
    </Detail>
  )
}

/**
 * The Copy for Venuta bookmark: dragged to your browser's bookmarks bar (or copied and saved as a
 * bookmark's address), then clicked on a music website where you're signed in.
 */
function Bookmark(props: { onPaste: () => void }) {
  // React won't render a javascript: address, so it's set on the element itself.
  const link = useRef<HTMLAnchorElement>(null)
  useEffect(() => link.current?.setAttribute('href', BOOKMARK_URL), [])
  const copy = async () => {
    await navigator.clipboard.writeText(BOOKMARK_URL)
    notify('Copied. In your browser, add a bookmark and paste this as its address (URL).')
  }
  return (
    <div className="grid w-full gap-3 text-[13px] leading-relaxed text-ink/75">
      <ol className="grid gap-2">
        <li className="flex flex-wrap items-center gap-x-3 gap-y-2">
          <span className="grid h-5 w-5 shrink-0 place-items-center rounded-full bg-ink/[0.08] text-[11px] font-semibold text-ink/60">1</span>
          <span>Drag this to your browser's bookmarks bar:</span>
          <a
            ref={link}
            onClick={(e) => e.preventDefault()}
            draggable
            title="Drag me to your browser's bookmarks bar"
            className="cursor-grab rounded-full bg-[color-mix(in_oklab,var(--accent)_18%,transparent)] px-3 py-1 font-semibold text-accent-ink shadow-[inset_0_0_0_1px_color-mix(in_oklab,var(--accent)_45%,transparent)] active:cursor-grabbing"
          >
            Copy for Venuta
          </a>
          <button onClick={() => void copy()} className="btn-ghost btn-sm">
            or copy it
          </button>
        </li>
        <li className="flex gap-3">
          <span className="mt-[3px] grid h-5 w-5 shrink-0 place-items-center rounded-full bg-ink/[0.08] text-[11px] font-semibold text-ink/60">2</span>
          <span>On the music website, signed in, open the playlist or your liked songs and click the bookmark. It loads the whole list and copies it.</span>
        </li>
        <li className="flex gap-3">
          <span className="mt-[3px] grid h-5 w-5 shrink-0 place-items-center rounded-full bg-ink/[0.08] text-[11px] font-semibold text-ink/60">3</span>
          <span>When it says Copied, come back and paste.</span>
        </li>
      </ol>
      <div>
        <button onClick={props.onPaste} className="btn">
          Paste what you copied
        </button>
      </div>
    </div>
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

type Show = 'check' | 'found' | 'online' | 'get'

function ReviewCard(props: { list: PreviewPlaylist; pick: Pick | undefined; onChange: (p: Partial<Pick>) => void }) {
  const { list: l, pick } = props
  const [open, setOpen] = useState(false)
  const [show, setShow] = useState<Show>(l.close ? 'check' : 'found')
  const [more, setMore] = useState(false)
  if (!pick) return null
  const rows = l.items
    .map((it, i) => ({ it, i }))
    .filter(({ it }) =>
      show === 'check' ? it.grade === 'close' : show === 'found' ? it.grade === 'exact' : show === 'online' ? !it.found && !!it.online : !it.found && !it.online,
    )
  const shown = more ? rows : rows.slice(0, 150)
  const accepted = l.close - [...pick.rejected].filter((i) => l.items[i]?.grade === 'close').length
  const onlineN = l.online ?? l.items.filter((it) => !it.found && it.online).length
  const toGet = l.missing + (l.close - accepted)
  const t = l.target
  const nameClash = l.exists && pick.include && !t

  return (
    <section className={`glass rounded-[20px] transition-opacity ${pick.include ? '' : 'opacity-60'}`} aria-label={l.name}>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2 p-4">
        <Switch on={pick.include} onChange={(include) => props.onChange({ include })} label={`Import ${l.name}`} />
        <input
          value={pick.name}
          onChange={(e) => props.onChange({ name: e.target.value })}
          onBlur={() => !pick.name.trim() && props.onChange({ name: l.name })}
          aria-label="Name for this playlist"
          disabled={!pick.include || (pick.mode === 'update' && !!t)}
          className="min-w-[180px] flex-1 rounded-lg bg-transparent px-2 py-1 text-[15.5px] font-semibold outline-none transition-colors -mx-2 hover:bg-ink/[0.05] focus:bg-ink/[0.07] focus:ring-1 focus:ring-[color-mix(in_oklab,var(--accent)_60%,transparent)]"
        />
        <span className="flex flex-wrap items-center gap-1.5 text-[12px]">
          <span className="rounded-full bg-ink/[0.07] px-2.5 py-1 text-ink/65">{l.source}</span>
          <span className="rounded-full bg-[color-mix(in_oklab,var(--accent)_18%,transparent)] px-2.5 py-1 font-medium text-accent-ink">{l.exact} found</span>
          {l.close > 0 && <span className="rounded-full bg-ink/[0.07] px-2.5 py-1 text-ink/70">{l.close} close {l.close === 1 ? 'match' : 'matches'}</span>}
          {onlineN > 0 && <span className="rounded-full bg-ink/[0.07] px-2.5 py-1 text-ink/70">{onlineN} online</span>}
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
      {!l.items.length && (
        <p className="px-4 pb-4 text-[12.5px] text-ink/50">
          {l.source === 'Venuta backup' ? 'An empty playlist: it comes back empty.' : 'This playlist is empty, so there is nothing to import.'}
        </p>
      )}
      {t && pick.include && (
        <div className="border-t border-edge px-4 py-3 text-[13px] text-ink/75">
          <div className="flex flex-wrap items-center gap-3">
            <span className="min-w-0 flex-1">
              {t.linked ? `This updates your “${t.name}”, which came from here.` : `You already have a playlist called “${t.name}”.`}
            </span>
            <Segmented
              label="What to do with it"
              size="sm"
              value={pick.mode}
              onChange={(mode) => props.onChange({ mode, name: mode === 'update' ? t.name : pick.name })}
              options={[
                { value: 'update', label: 'Update it' },
                { value: 'both', label: 'Keep both' },
                { value: 'replace', label: 'Replace it' },
              ]}
            />
          </div>
          <p className="mt-2 text-[12.5px] leading-relaxed text-ink/60">
            {pick.mode === 'update'
              ? updateLine(t, l.source)
              : pick.mode === 'replace'
                ? `Your “${t.name}” goes to Set aside, and this one takes its place. Songs you added to it are not carried over.`
                : 'This one is saved beside yours, with a number added to its name.'}
          </p>
        </div>
      )}
      {nameClash && (
        <div className="flex flex-wrap items-center gap-3 border-t border-edge px-4 py-3 text-[13px] text-ink/70">
          <span className="min-w-0 flex-1">You already have a playlist called “{l.name}”.</span>
          <Segmented
            label="When the name is taken"
            size="sm"
            value={pick.mode === 'replace' ? 'replace' : 'both'}
            onChange={(v) => props.onChange({ mode: v })}
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
                  ...(onlineN ? [{ value: 'online' as const, label: `Online ${onlineN}` }] : []),
                  ...(l.missing ? [{ value: 'get' as const, label: `To get ${l.missing}` }] : []),
                ]}
              />
              {show === 'online' && (
                <p className="mt-2.5 text-[12.5px] text-ink/50">Songs you don't have play from the online catalog, in their places, until you have the file.</p>
              )}
              {show === 'get' && (
                <p className="mt-2.5 text-[12.5px] text-ink/50">The online catalog wasn't sure about these. They keep their places and wait on To get; you can pick an online song for each in the playlist.</p>
              )}
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
        ) : props.item.online ? (
          <>
            <span className="block truncate">
              <span className="text-ink/40" aria-hidden>
                →{' '}
              </span>
              {props.item.online.title}
            </span>
            <span className="block truncate text-[12px] text-ink/50">
              {[(props.item.online.artists ?? []).join(', '), 'plays online'].filter(Boolean).join(', ')}
            </span>
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
