import { AnimatePresence, LayoutGroup, MotionConfig, motion } from 'motion/react'
import { Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { Backdrop } from './components/Backdrop'
import { ThemeFx } from './components/ThemeFx'
import { Rail, type Section } from './components/Rail'
import { TitleBar } from './components/TitleBar'
import { CommandPalette, ShortcutSheet, type Command } from './components/CommandPalette'
import { Toast } from './components/Toast'
import {
  IconActivity,
  IconCards,
  IconHealth,
  IconInbox,
  IconListen,
  IconTag,
  IconSettings,
} from './components/Icons'
import {
  cancelDeepCheck,
  cancelLookup,
  deepCheck,
  ejectCard,
  listCards,
  syncCard,
  lookupOnline,
  inApp,
  setCloseAction,
  chooseHome,
  checkMusicFolder,
  getSettings,
  libraryHome,
  setOpenFolder,
  libraryHealth,
  scanLibrary,
  playlistShelf,
  type HealthReport,
  type PlaylistSummary,
} from './lib/engine'
import { coverFor, loadCovers, refreshAccent, setPlayingAccent, useCovers } from './lib/covers'
import { track } from './lib/jobs'
import { dismissNotice, notify, useNotice } from './lib/notify'
import * as player from './lib/player'
import { connectPlayer, resumePlayer } from './lib/player'
import { NowPlayingBar } from './components/player/NowPlayingBar'
import { QueueDrawer } from './components/player/QueueDrawer'
import { NowPlaying } from './components/player/NowPlaying'
import { adoptPrefs, getPrefs, motionSetting, resolvedTheme, setPrefs, usePrefs, type Prefs, type Surface } from './lib/prefs'
import { DeepProgressBar } from './screens/libraryParts'
import { MissingFolder, NoLibrary, NoMusic, ReadFailed } from './components/FolderStates'
import { MusicScreen } from './screens/MusicScreen'
import { ListenScreen, showMusicTab } from './screens/ListenScreen'
import { HealthScreen } from './screens/HealthScreen'
import { showPlaylists, type PlaylistsPlace } from './screens/PlaylistsScreen'
import { AddToPlaylist } from './components/AddToPlaylist'
import { ConfirmHost, chooseAsk } from './components/Confirm'
import { settingsSection } from './screens/settingsPage'
import { checkAtStart } from './lib/updates'
// Places opened now and then load the first time they are opened, so the start is quicker.
const InboxScreen = lazy(() => import('./screens/InboxScreen').then((m) => ({ default: m.InboxScreen })))
const CardsScreen = lazy(() => import('./screens/CardsScreen').then((m) => ({ default: m.CardsScreen })))
const ActivityScreen = lazy(() => import('./screens/ActivityScreen').then((m) => ({ default: m.ActivityScreen })))
const SettingsScreen = lazy(() => import('./screens/SettingsScreen').then((m) => ({ default: m.SettingsScreen })))
const SetupScreen = lazy(() => import('./screens/SetupScreen').then((m) => ({ default: m.SetupScreen })))

// Before the open folder was kept with the library, this computer remembered it; read once.
const ROOT_KEY = 'library-root'
const store = {
  get: () => {
    try {
      return localStorage.getItem(ROOT_KEY) ?? ''
    } catch {
      return ''
    }
  },
}

/** Same folder, whatever the slashes or letter case (Windows paths). */
const normPath = (p: string) => p.replace(/[\\/]+/g, '/').replace(/\/$/, '').toLowerCase()
const samePath = (a: string | null | undefined, b: string | null | undefined) => !!a && !!b && normPath(a) === normPath(b)

// "listen" is the Music home (listening); "music" is the album wall for details (Metadata).
// Playlists live in Music (its Playlists tab).
type Place = 'listen' | 'music' | 'health' | 'inbox' | 'devices' | 'activity' | 'settings'
const ORDER: Place[] = ['listen', 'music', 'health', 'inbox', 'devices', 'activity', 'settings']
/** The places on the top of the rail, in order (Ctrl+1 … Ctrl+5). */
const MAIN = 5

/**
 * The window's backdrop: the playing song's cover whenever there is one, so the backdrop and the
 * accent always come from the same art; the library's covers when nothing is loaded.
 */
function NowBackdrop({ covers }: { covers: string[] }) {
  const dir = player.usePlayerValue((s) => player.current(s)?.dir ?? '')
  const cover = useCovers(320)
  const art = dir ? cover(dir) : undefined
  // The accent follows what's playing (see lib/covers.ts).
  useEffect(() => setPlayingAccent(art?.accent), [art?.accent])
  // And finds its right variant again when the look changes.
  const look = usePrefs()
  useEffect(() => refreshAccent(), [look.style, look.theme, look.surface, look.accentMode, look.accent, look.intensity])
  return <Backdrop covers={covers} focus={art ? art.src : null} />
}

export default function App() {
  const prefs = usePrefs()
  // The folder showing: your library, or music being brought in. `home` is your library,
  // where everything Venuta builds and knows is kept.
  const [root, setRoot] = useState(inApp() ? '' : 'D:/Collection')
  const [home, setHome] = useState<string | null>(inApp() ? null : 'D:/Music Library')
  // Your albums live in the library's Music folder; playlists, new music and set-aside copies
  // sit beside it.
  const libMusic = home ? `${home.replace(/[\\/]+$/, '')}/Music` : null
  const [suggestion, setSuggestion] = useState('D:/Music Library')
  const [ready, setReady] = useState(!inApp())
  const [missing, setMissing] = useState('')
  const [report, setReport] = useState<HealthReport | null>(null)
  // Reading the folder failed (not a missing folder): its screens offer to try again.
  const [readError, setReadError] = useState('')
  const [busy, setBusy] = useState('')
  const [error, setError] = useState('')
  const [active, setActive] = useState<Place>('listen')
  const [direction, setDirection] = useState(1)
  const [backdrop, setBackdrop] = useState<string[]>([])
  const [deep, setDeep] = useState<{ done: number; total: number } | null>(null)
  const [lookup, setLookup] = useState<{ done: number; total: number; note?: string } | null>(null)
  const [notice, setNotice] = useState('')
  // Notices from anywhere in the app (added to a playlist, removed with Undo…).
  const shared = useNotice()
  const [palette, setPalette] = useState(false)
  // Your playlists, for the palette (read when it opens).
  const [shelf, setShelf] = useState<PlaylistSummary[]>([])
  useEffect(() => {
    if (!palette || !root) return
    playlistShelf(root)
      .then(setShelf)
      .catch(() => {})
  }, [palette, root])
  const [sheet, setSheet] = useState(false)
  const [inboxCount, setInboxCount] = useState(0)
  // The guided set-up: shown until a library is chosen, and on request.
  const [setup, setSetup] = useState(false)
  const search = useRef<HTMLInputElement>(null)
  // Music has its own: both screens are briefly mounted while one slides out, so one shared ref
  // would be cleared by the screen leaving.
  const listenSearch = useRef<HTMLInputElement>(null)
  // An album to open on the Music home (from the palette), once it is showing.
  const [listenAlbum, setListenAlbum] = useState<string | null>(null)
  // The player's big view (and which side panel) and the queue drawer.
  const [nowOpen, setNowOpen] = useState(false)
  const [nowPanel, setNowPanel] = useState<'lyrics' | 'next' | 'signal' | 'sound'>('lyrics')
  const [queueOpen, setQueueOpen] = useState(false)
  // Counts choosing the open place again in the rail (it goes back to its start).
  const [again, setAgain] = useState(0)
  // Each visit to a place is its own layout namespace: a sliding pill inside a page never
  // flies in from where it was on an earlier visit.
  const [visit, setVisit] = useState(0)
  const closeQueue = useCallback(() => setQueueOpen(false), [])
  const closeNow = useCallback(() => setNowOpen(false), [])
  const closeNotice = useCallback(() => setNotice(''), [])
  const closeError = useCallback(() => setError(''), [])

  const refresh = useCallback(async (dir: string, rescan: boolean): Promise<HealthReport | null> => {
    try {
      const r = await track({ id: 'scan', label: 'Reading your music', unit: 'files' }, async (update, relabel) => {
        // Walk, read tags, save: each stage gets its own words and its own count.
        const read = async () => {
          let stage = ''
          await scanLibrary(dir, (p) => {
            if (p.stage !== stage) {
              stage = p.stage
              const label =
                p.stage === 'finding' ? 'Finding your music' : p.stage === 'reading' ? 'Reading song details' : 'Saving what was read'
              relabel(label)
              setBusy(`${label}…`)
            }
            update(p.done, p.stage === 'saving' ? 0 : p.total)
          })
        }
        if (rescan) await read()
        relabel('Checking your files')
        setBusy('Checking your files…')
        let r = await libraryHealth(dir)
        // A library that was never read shows as empty: read it once, then report.
        if (!rescan && r.summary.tracks === 0) {
          await read()
          relabel('Checking your files')
          r = await libraryHealth(dir)
        }
        return r
      })
      setReport(r)
      setError('')
      setReadError('')
      setBusy('')
      // Covers: a spread of albums for the background first, then everything for the wall.
      const dirs = r.albums.filter((a) => a.cover_track).map((a) => a.dir)
      await track({ id: 'covers', label: 'Making album covers', unit: 'albums' }, async (update) => {
        const step = Math.max(1, Math.floor(dirs.length / 24))
        const spread = dirs.filter((_, i) => i % step === 0).slice(0, 24)
        await loadCovers(dir, spread)
        const loaded = spread.map((d) => coverFor(d)).filter((c) => !!c)
        setBackdrop(loaded.map((c) => c.src))
        await loadCovers(dir, dirs, 320, update)
      })
      return r
    } catch (e) {
      setBusy('')
      // A folder that has gone (drive unplugged, renamed) gets its own screen, not an error.
      if (/does not exist or is not a directory/.test(String(e))) setMissing(dir)
      else {
        setError(String(e))
        setReadError(String(e).replace(/^Error: /, ''))
      }
      return null
    }
  }, [])

  // The player is app-wide: hear its state from the start, whatever screen is open.
  useEffect(() => connectPlayer(), [])
  // Once the folder is read, the queue from last time comes back, paused where you stopped.
  useEffect(() => {
    if (report && root) resumePlayer(root)
  }, [report, root])

  // On start: where the library is, which folder was open, and the look saved with it.
  useEffect(() => {
    if (!inApp()) return
    void (async () => {
      try {
        const [h, s] = await Promise.all([libraryHome(), getSettings()])
        adoptPrefs(s.appearance as Partial<Prefs> | null)
        setSuggestion(h.suggestion)
        setHome(h.library)
        if (!h.library) setSetup(true)
        else setRoot(s.open_folder || store.get() || `${h.library.replace(/[\\/]+$/, '')}/Music`)
      } catch (e) {
        setError(String(e))
      } finally {
        setReady(true)
      }
    })()
  }, [])

  // Cards set to sync on insert: sync when they appear, whatever screen is open.
  const present = useRef<Set<string> | null>(null)
  useEffect(() => {
    if (!inApp() || !root) return
    const t = window.setInterval(async () => {
      const cards = await listCards().catch(() => [])
      const now = new Set(cards.filter((c) => c.card).map((c) => c.volume.root))
      const before = present.current
      present.current = now
      if (!before) return // first look: only react to cards inserted from now on
      for (const c of cards) {
        if (!c.card?.auto_sync || before.has(c.volume.root)) continue
        setNotice(`Syncing ${c.card.name}…`)
        try {
          const name = c.card.name
          const r = await track({ id: `sync:${c.volume.root}`, label: `Syncing ${name}`, unit: 'files' }, (update) =>
            syncCard(root, c.volume.root, (done, total) => update(done, total)),
          )
          await ejectCard(c.volume.root).catch(() => {})
          setNotice(`${c.card.name} is up to date (${r.copied} copied, ${r.patched} updated, ${r.deleted} removed) and safe to remove.`)
        } catch (e) {
          setError(String(e))
        }
      }
    }, 5000)
    return () => window.clearInterval(t)
  }, [root])

  // On launch, show the last library straight from its index; a newly chosen folder is read
  // afresh first. One refresh per change of folder, never two racing each other.
  const readNext = useRef(false)
  useEffect(() => {
    if (!root) return
    const rescan = readNext.current
    readNext.current = false
    void refresh(root, rescan)
  }, [root, refresh])

  const go = useCallback(
    (id: Place) => {
      // Going anywhere (even to the place underneath) closes the full-window views over it.
      setNowOpen(false)
      setQueueOpen(false)
      const from = ORDER.indexOf(active)
      const to = ORDER.indexOf(id)
      if (to < 0 || to === from) return
      setDirection(to > from ? 1 : -1)
      setVisit((n) => n + 1)
      setActive(id)
    },
    [active],
  )

  // A new version: looked for quietly once a day, offered with a notice; installed only when asked.
  useEffect(() => {
    if (!inApp()) return
    const t = window.setTimeout(() => {
      void checkAtStart().then((info) => {
        if (info)
          notify(`Venuta ${info.version} is ready to install.`, {
            label: 'See what’s new',
            run: () => {
              settingsSection('about')
              go('settings')
            },
          })
      })
    }, 8000)
    return () => window.clearTimeout(t)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // The first time the window is closed, ask what closing should do (Rust holds the window open).
  useEffect(() => {
    if (!inApp()) return
    let off: (() => void) | undefined
    let live = true
    void import('@tauri-apps/api/event')
      .then(({ listen }) =>
        listen('venuta://close-asked', async () => {
          const a = await chooseAsk({
            title: 'Keep playing when you close the window?',
            text: 'Venuta can stay in the tray, by the clock: the music goes on and your media keys still work. Click its icon to come back. You can change this later in Settings, Startup and tray.',
            confirm: 'Keep playing in the tray',
            other: 'Quit Venuta',
          })
          if (a !== 'no') await setCloseAction(a === 'yes' ? 'tray' : 'quit', true)
        }),
      )
      .then((u) => (live ? (off = u) : u()))
    return () => {
      live = false
      off?.()
    }
  }, [])

  // Playlists, in Music, from anywhere (the Music home's shelf, a notice's Open, the palette).
  const openPlaylists = useCallback(
    (place: PlaylistsPlace = { kind: 'home' }) => {
      showPlaylists(place)
      showMusicTab('playlists')
      go('listen')
    },
    [go],
  )
  const openPlaylist = useCallback((name: string) => openPlaylists({ kind: 'playlist', name }), [openPlaylists])

  // "Playing from …" (the queue, Now Playing): back to that playlist or album.
  useEffect(() => {
    const on = (e: Event) => {
      const f = (e as CustomEvent<player.From>).detail
      if (f.kind === 'playlist') openPlaylist(f.ref)
      else if (f.kind === 'album') {
        go('listen')
        setListenAlbum(f.ref)
      } else go('listen')
    }
    window.addEventListener('venuta:open-from', on)
    return () => window.removeEventListener('venuta:open-from', on)
  }, [openPlaylist, go])

  const deepRunning = useRef(false)
  async function runDeepCheck() {
    if (deepRunning.current) return
    deepRunning.current = true
    setDeep({ done: 0, total: 0 })
    try {
      await track({ id: 'deep', label: 'Measuring loudness and checking quality', unit: 'songs', cancel: () => void cancelDeepCheck() }, (update) =>
        deepCheck(root, (done, total) => {
          setDeep({ done, total })
          update(done, total)
        }),
      )
      await refresh(root, false)
    } catch (e) {
      setError(String(e))
    } finally {
      setDeep(null)
      deepRunning.current = false
    }
  }

  /**
   * New music came in: read it, and measure it in the background when the rest of the library
   * is measured already (the first full measurement stays your choice, in Health).
   */
  async function afterImport() {
    const r = await refresh(root, false)
    const unmeasured = r?.issues.find((i) => i.id === 'no_replaygain')?.tracks ?? 0
    if (r && unmeasured > 0 && unmeasured <= Math.max(300, r.summary.tracks * 0.1)) void runDeepCheck()
  }

  async function runLookup() {
    setLookup({ done: 0, total: 0 })
    try {
      const r = await track(
        { id: 'lookup', label: 'Looking up details online', unit: 'steps', cancel: () => void cancelLookup() },
        (update) =>
          lookupOnline(root, (done, total, note) => {
            setLookup({ done, total, note })
            update(done, total, note)
          }),
      )
      const nothing = r.albums_looked_up === 0 && r.lyrics_checked === 0 && !r.discogs_matches && !r.identified
      setNotice(
        nothing && !r.cancelled
          ? 'Everything is already looked up; nothing new to find. Add music, or keys in Settings, Online sources, to find more.'
          : r.cancelled
          ? 'Stopped. What was found is kept; look up again to continue.'
          : `Looked up ${r.albums_looked_up} albums (${r.musicbrainz_matches} on MusicBrainz, ${r.deezer_matches} on Deezer, ${r.apple_matches} on Apple${r.discogs_matches ? `, ${r.discogs_matches} on Discogs` : ''}) and found lyrics for ${r.lyrics_found} of ${r.lyrics_checked} songs${r.identified ? `; recognised ${r.identified} files without tags by their sound` : ''}. Organize shows what changes.`,
      )
    } catch (e) {
      setError(String(e))
    } finally {
      setLookup(null)
    }
  }

  async function chooseRoot() {
    const picked = inApp() ? await open({ directory: true, title: 'Choose the folder that holds your music' }) : null
    if (typeof picked !== 'string') return
    try {
      await checkMusicFolder(picked)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
      return
    }
    openLibrary(picked)
  }

  function openLibrary(path: string) {
    void setOpenFolder(path).catch(() => {})
    setMissing('')
    setReport(null)
    setBackdrop([])
    // Show the music filling in rather than leaving you on the screen you chose it from.
    go('listen')
    if (samePath(path, root)) void refresh(path, true)
    else {
      readNext.current = true
      setRoot(path)
    }
  }

  const toggleTheme = useCallback(() => setPrefs({ theme: resolvedTheme() === 'dark' ? 'light' : 'dark' }), [])

  // Everything the palette can do.
  const commands: Command[] = useMemo(() => {
    const goTo = (p: Place, label: string, keys: string, keywords = ''): Command => ({
      id: `go-${p}`,
      group: 'Go to',
      label,
      keys,
      keywords,
      run: () => go(p),
    })
    const surfaces: [Surface, string][] = [
      ['glass', 'Glass'],
      ['frosted', 'Frosted'],
      ['solid', 'Solid'],
      ['amoled', 'AMOLED (true black)'],
    ]
    const list: Command[] = [
      goTo('listen', 'Music', 'Ctrl+1', 'listen play home albums artists songs player'),
      goTo('music', 'Metadata', 'Ctrl+2', 'tags details edit covers album wall'),
      goTo('health', 'Health', 'Ctrl+3', 'status quality metadata completeness problems tools'),
      goTo('inbox', 'Organize', 'Ctrl+4', 'inbox questions decisions review new music incoming manage library naming'),
      { id: 'playlists', group: 'Go to', label: 'Playlists', keywords: 'music smart want-list to get', run: () => openPlaylists() },
      {
        id: 'playlist-new',
        group: 'Actions',
        label: 'New playlist',
        keywords: 'make create mix',
        run: () => openPlaylists({ kind: 'new' }),
      },
      {
        id: 'playlist-import',
        group: 'Actions',
        label: 'Import playlists',
        keywords: 'spotify apple music youtube deezer tidal amazon m3u bring over transfer update',
        run: () => openPlaylists({ kind: 'import' }),
      },
      {
        id: 'playlist-wanted',
        group: 'Go to',
        label: 'To get (albums your playlists want)',
        keywords: 'want-list wanted missing buy',
        run: () => openPlaylists({ kind: 'wanted' }),
      },
      ...shelf.map(
        (p): Command => ({
          id: `playlist:${p.name}`,
          group: 'Playlists',
          label: p.name,
          keywords: `playlist ${p.source ?? ''}`,
          run: () => openPlaylist(p.name),
        }),
      ),
      goTo('devices', 'Devices', 'Ctrl+5', 'cards sd players sync'),
      goTo('activity', 'Activity', 'Ctrl+H', 'history undo builds'),
      goTo('settings', 'Settings', 'Ctrl+,', 'preferences options keys'),
      { id: 'search', group: 'Actions', label: 'Search your music', keys: '/', run: () => focusSearch() },
      { id: 'play-toggle', group: 'Player', label: 'Play or pause', keys: 'Space', keywords: 'resume stop', run: () => void player.toggle() },
      { id: 'mini', group: 'Player', label: 'Mini player', keys: 'Ctrl+M', keywords: 'small window compact on top', run: () => void player.miniPlayer(true) },
      { id: 'play-next', group: 'Player', label: 'Next song', keys: 'Ctrl+→', keywords: 'skip forward', run: () => void player.next() },
      { id: 'play-prev', group: 'Player', label: 'Previous song', keys: 'Ctrl+←', keywords: 'back', run: () => void player.previous() },
      {
        id: 'play-shuffle',
        group: 'Player',
        label: 'Shuffle on or off',
        keywords: 'random order',
        run: () => void player.setShuffle(!player.getPlayer().shuffle),
      },
      { id: 'play-now', group: 'Player', label: 'Open Now Playing', keywords: 'big cover full', run: () => setNowOpen(true) },
      {
        id: 'play-sound',
        group: 'Player',
        label: 'Sound: equalizer, levelling, sleep timer',
        keywords: 'eq bass treble replaygain limiter sleep timer stop after',
        run: () => {
          setNowPanel('sound')
          setNowOpen(true)
        },
      },
      { id: 'play-queue', group: 'Player', label: 'Show the queue', keys: 'Q', keywords: 'up next', run: () => setQueueOpen(true) },
      {
        id: 'play-lyrics',
        group: 'Player',
        label: 'Show lyrics',
        keys: 'L',
        keywords: 'words karaoke',
        run: () => {
          setNowPanel('lyrics')
          setNowOpen(true)
        },
      },
      {
        id: 'quality',
        group: 'Actions',
        label: 'Check quality',
        keywords: 'deep check integrity lossy upsampled loudness',
        run: () => {
          go('health')
          void runDeepCheck()
        },
      },
      { id: 'lookup', group: 'Actions', label: 'Look up details online', keywords: 'musicbrainz covers lyrics credits years', run: () => void runLookup() },
      { id: 'rescan', group: 'Actions', label: 'Rescan your music', keywords: 'refresh reload', run: () => void refresh(root, true) },
      { id: 'choose', group: 'Actions', label: 'Choose the folder with your music', keywords: 'library open', run: () => void chooseRoot() },
      { id: 'setup', group: 'Actions', label: 'Set up your library', keywords: 'first run guide wizard start migrate', run: () => setSetup(true) },
      { id: 'shortcuts', group: 'Help', label: 'Show keyboard shortcuts', keys: '?', run: () => setSheet(true) },
      {
        id: 'theme-toggle',
        group: 'Look and feel',
        label: resolvedTheme() === 'dark' ? 'Switch to light' : 'Switch to dark',
        keys: 'Ctrl+Shift+L',
        keywords: 'theme mode',
        run: toggleTheme,
      },
      { id: 'theme-system', group: 'Look and feel', label: 'Match Windows theme', keywords: 'system auto', run: () => setPrefs({ theme: 'system' }) },
      ...surfaces.map(([s, label]) => ({
        id: `surface-${s}`,
        group: 'Look and feel',
        label: `Surface: ${label}`,
        keywords: 'transparent glass blur opaque black',
        run: () => setPrefs({ surface: s, ...(s === 'amoled' ? { theme: 'dark' as const } : {}) }),
      })),
      {
        id: 'accent-album',
        group: 'Look and feel',
        label: 'Colour from the music',
        keywords: 'accent color dynamic album lumen',
        run: () => setPrefs({ accentMode: 'album' }),
      },
      {
        id: 'accent-neutral',
        group: 'Look and feel',
        label: 'Neutral colour (no accent)',
        keywords: 'accent color monochrome white grey lumen',
        run: () => setPrefs({ accentMode: 'neutral' }),
      },
      {
        id: 'backdrop',
        group: 'Look and feel',
        label: getPrefs().backdrop ? 'Hide the album background' : 'Show the album background',
        keywords: 'artwork blur wallpaper',
        run: () => setPrefs({ backdrop: !getPrefs().backdrop }),
      },
    ]
    for (const a of report?.albums ?? []) {
      list.push({
        id: `album-${a.dir}`,
        group: 'Albums',
        label: a.title,
        detail: `${a.album_artist}${a.year ? `, ${a.year}` : ''}`,
        run: () => {
          go('listen')
          setListenAlbum(a.dir)
        },
      })
    }
    return list
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [report, root, go, prefs, toggleTheme, shelf, openPlaylists, openPlaylist])

  function focusSearch() {
    // Music and Metadata each have a search: the one showing, or Music from anywhere else.
    if (active !== 'listen' && active !== 'music') go('listen')
    const target = active === 'music' ? search : listenSearch
    window.setTimeout(() => target.current?.focus(), 80)
  }

  // Global shortcuts.
  useEffect(() => {
    const typing = (t: EventTarget | null) =>
      t instanceof HTMLElement && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)
    const onKey = (e: KeyboardEvent) => {
      const k = e.key.toLowerCase()
      if (e.ctrlKey && k === 'k') {
        e.preventDefault()
        setPalette((p) => !p)
      } else if (e.ctrlKey && e.shiftKey && k === 'l') {
        e.preventDefault()
        toggleTheme()
      } else if (e.ctrlKey && k === ',') {
        e.preventDefault()
        go('settings')
      } else if (e.ctrlKey && k === 'h') {
        e.preventDefault()
        go('activity')
      } else if (e.ctrlKey && /^[1-5]$/.test(k)) {
        e.preventDefault()
        go(ORDER[Number(k) - 1])
      } else if (!e.ctrlKey && !e.altKey && !typing(e.target)) {
        if (e.key === '/') {
          e.preventDefault()
          focusSearch()
        } else if (e.key === '?') {
          e.preventDefault()
          setSheet(true)
        }
      }
      if (e.key === 'Escape') {
        setSheet(false)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [go, toggleTheme])

  // Two-finger horizontal swipe on a trackpad moves between the four places.
  const swipe = useRef({ acc: 0, lock: 0 })
  const onWheel = (e: React.WheelEvent) => {
    if (Math.abs(e.deltaX) < Math.abs(e.deltaY) * 1.5) return
    const s = swipe.current
    const now = performance.now()
    if (now < s.lock) return
    s.acc += e.deltaX
    if (Math.abs(s.acc) > 160) {
      const places = ORDER.slice(0, MAIN)
      const i = places.indexOf(active)
      const next = i >= 0 ? places[i + (s.acc > 0 ? 1 : -1)] : undefined
      s.acc = 0
      s.lock = now + 650
      if (next) go(next)
    }
  }

  const top: Section[] = [
    { id: 'listen', label: 'Music', icon: IconListen, keys: 'Ctrl+1' },
    { id: 'music', label: 'Metadata', icon: IconTag, keys: 'Ctrl+2' },
    { id: 'health', label: 'Health', icon: IconHealth, keys: 'Ctrl+3' },
    { id: 'inbox', label: 'Organize', icon: IconInbox, keys: 'Ctrl+4', badge: inboxCount },
    { id: 'devices', label: 'Devices', icon: IconCards, keys: 'Ctrl+5' },
  ]
  const bottom: Section[] = [
    { id: 'activity', label: 'Activity', icon: IconActivity, keys: 'Ctrl+H' },
    { id: 'settings', label: 'Settings', icon: IconSettings, keys: 'Ctrl+,' },
  ]

  const screen = (() => {
    if (setup && active !== 'settings')
      return (
        <SetupScreen
          root={root}
          home={home}
          music={libMusic}
          suggestion={suggestion}
          report={report}
          busy={busy}
          lookup={lookup}
          onChooseHome={async (path) => {
            await chooseHome(path)
            setHome(path)
          }}
          onChooseMusic={chooseRoot}
          onStartEmpty={() => libMusic && openLibrary(libMusic)}
          onLookup={runLookup}
          onCancelLookup={() => void cancelLookup()}
          onDone={() => {
            setSetup(false)
            go('inbox')
          }}
          onClose={root && home ? () => setSetup(false) : undefined}
        />
      )
    if (!ready) return null
    if (missing && samePath(missing, root) && active !== 'settings')
      return (
        <MissingFolder
          path={missing}
          isLibrary={samePath(missing, libMusic)}
          onRetry={() => {
            setMissing('')
            void refresh(root, true)
          }}
          onChoose={() => void chooseRoot()}
          onLibrary={libMusic && !samePath(missing, libMusic) ? () => openLibrary(libMusic) : undefined}
        />
      )
    if (!root && active !== 'settings') return <NoLibrary onSetup={() => setSetup(true)} />
    if (!report && readError && active !== 'settings')
      return (
        <ReadFailed
          path={root}
          message={readError}
          onRetry={() => {
            setReadError('')
            setError('')
            void refresh(root, true)
          }}
          onChoose={() => void chooseRoot()}
        />
      )
    switch (active) {
      case 'listen':
        if (report && report.summary.tracks === 0)
          return (
            <NoMusic path={root} isLibrary={samePath(root, libMusic)} onAdd={() => go('inbox')} onChoose={() => void chooseRoot()} />
          )
        return (
          <ListenScreen
            root={root}
            report={report}
            busy={busy}
            searchRef={listenSearch}
            openAlbum={listenAlbum}
            onOpenedAlbum={() => setListenAlbum(null)}
            onEditDetails={() => go('music')}
            onOpenPlaylist={openPlaylist}
            onPlaylists={openPlaylists}
            reset={again}
          />
        )
      case 'music':
        if (report && report.summary.tracks === 0)
          return (
            <NoMusic path={root} isLibrary={samePath(root, libMusic)} onAdd={() => go('inbox')} onChoose={() => void chooseRoot()} />
          )
        return <MusicScreen root={root} report={report} busy={busy} searchRef={search} />
      case 'health':
        return (
          <HealthScreen
            root={root}
            report={report}
            busy={busy}
            onLookup={runLookup}
            onCheck={runDeepCheck}
            onRescan={() => void refresh(root, true)}
            onOrganize={() => go('inbox')}
            running={{ lookup: !!lookup, check: !!deep }}
            progress={
              <>
                <AnimatePresence>
                  {deep && <DeepProgressBar deep={deep} label="Measuring loudness and checking quality" unit="songs" onCancel={() => void cancelDeepCheck()} />}
                </AnimatePresence>
                <AnimatePresence>
                  {lookup && (
                    <DeepProgressBar deep={lookup} label="Looking up details online" unit="steps" onCancel={() => void cancelLookup()} />
                  )}
                </AnimatePresence>
              </>
            }
          />
        )
      case 'inbox':
        return (
          <InboxScreen
            root={root}
            home={libMusic}
            onChooseMusic={() => void chooseRoot()}
            onImported={() => void afterImport()}
            onCount={setInboxCount}
            onSwitchLibrary={(path) => {
              openLibrary(path)
              go('listen')
              setNotice(`Now showing your new library in ${path}.`)
            }}
          />
        )
      case 'devices':
        return <CardsScreen root={root} />
      case 'activity':
        return <ActivityScreen root={root} />
      case 'settings':
        return (
          <SettingsScreen
            root={root}
            home={home}
            music={libMusic}
            onChooseLibrary={chooseRoot}
            onOpenLibrary={() => libMusic && openLibrary(libMusic)}
            onChooseHome={async (path) => {
              if (root) await checkMusicFolder(root, path)
              await chooseHome(path)
              setHome(path)
              openLibrary(`${path.replace(/[\\/]+$/, '')}/Music`)
            }}
          />
        )
    }
  })()

  return (
    <MotionConfig reducedMotion={motionSetting(prefs)}>
      <div className="relative flex h-full flex-col" onWheel={onWheel}>
        <NowBackdrop covers={backdrop} />
        <ThemeFx />
        <TitleBar onPalette={() => setPalette(true)} />
        <div className="relative z-10 flex min-h-0 flex-1">
          <Rail
            top={top}
            bottom={bottom}
            active={setup ? '' : active}
            onSelect={(id) => {
              setSetup(false)
              // The place you're on, chosen again (and nothing over it): back to its start.
              if (id === active && !nowOpen) {
                setAgain((n) => n + 1)
                document.querySelector('main .thin-scroll')?.scrollTo({ top: 0, behavior: 'smooth' })
              }
              go(id as Place)
            }}
          />
          <div className="relative flex min-w-0 flex-1 flex-col">
          <main className="stage relative min-h-0 min-w-0 flex-1 overflow-hidden">
            {/* The page steps back while Now Playing is open; it keeps its place. */}
            <div
              inert={nowOpen}
              className="absolute inset-0 transition-[opacity,transform,filter] duration-300 ease-out"
              style={nowOpen ? { opacity: 0, transform: 'scale(0.985)', pointerEvents: 'none' } : undefined}
            >
            <AnimatePresence mode="popLayout" custom={direction} initial={false}>
              <motion.div
                key={active}
                custom={direction}
                // The rail is a column, so places move up and down with it: a short rise, quick.
                variants={{
                  enter: (d: number) => ({ y: d * 18, opacity: 0 }),
                  center: { y: 0, opacity: 1, transition: { type: 'spring', stiffness: 420, damping: 38, opacity: { duration: 0.18 } } },
                  exit: (d: number) => ({ y: d * -10, opacity: 0, transition: { duration: 0.12 } }),
                }}
                initial="enter"
                animate="center"
                exit="exit"
                className="place thin-scroll absolute inset-0 overflow-y-auto overflow-x-hidden px-10"
              >
                <LayoutGroup id={`${active}-${visit}`}>
                  <Suspense fallback={null}>{screen}</Suspense>
                </LayoutGroup>
              </motion.div>
            </AnimatePresence>
            </div>
            <Toast text={error ? '' : notice} onClose={closeNotice} />
            <Toast
              text={error || notice ? '' : (shared?.text ?? '')}
              action={shared?.action}
              stamp={shared?.id}
              onClose={dismissNotice}
            />
            <Toast text={error} tone="error" onClose={closeError} />
            <NowPlaying
              open={nowOpen}
              root={root}
              panel={nowPanel}
              onPanel={setNowPanel}
              onClose={closeNow}
              onAlbum={(dir) => {
                setNowOpen(false)
                setListenAlbum(dir)
                go('listen')
              }}
            />
            <QueueDrawer open={queueOpen} onClose={closeQueue} />
          </main>
          <NowPlayingBar
            expanded={nowOpen}
            queueOpen={queueOpen}
            onExpand={() => setNowOpen((o) => !o)}
            onQueue={() => setQueueOpen((o) => !o)}
            onLyrics={() => {
              setNowPanel('lyrics')
              setNowOpen((o) => !(o && nowPanel === 'lyrics'))
            }}
            onArtist={() => {
              setNowOpen(false)
              go('listen')
            }}
          />
          </div>
        </div>
        <CommandPalette open={palette} onClose={() => setPalette(false)} commands={commands} />
        {root && (
          <AddToPlaylist root={root} onOpen={openPlaylist} />
        )}
        <ConfirmHost />
        <ShortcutSheet open={sheet} onClose={() => setSheet(false)} />
      </div>
    </MotionConfig>
  )
}
