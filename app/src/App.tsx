import { AnimatePresence, MotionConfig, motion } from 'motion/react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { Backdrop } from './components/Backdrop'
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
  IconPlaylists,
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
  chooseHome,
  checkMusicFolder,
  getSettings,
  libraryHome,
  setOpenFolder,
  libraryHealth,
  scanLibrary,
  type HealthReport,
} from './lib/engine'
import { coverFor, loadCovers, setAccent } from './lib/covers'
import { track } from './lib/jobs'
import { adoptPrefs, getPrefs, motionSetting, resolvedTheme, setPrefs, usePrefs, type Prefs, type Surface } from './lib/prefs'
import { DeepProgressBar } from './screens/LibraryScreen'
import { MissingFolder, NoLibrary, NoMusic } from './components/FolderStates'
import { MusicScreen } from './screens/MusicScreen'
import { HealthScreen } from './screens/HealthScreen'
import { InboxScreen } from './screens/InboxScreen'
import { CardsScreen } from './screens/CardsScreen'
import { PlaylistsScreen } from './screens/PlaylistsScreen'
import { ActivityScreen } from './screens/ActivityScreen'
import { SettingsScreen } from './screens/SettingsScreen'
import { SetupScreen } from './screens/SetupScreen'

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
type Place = 'listen' | 'music' | 'health' | 'inbox' | 'playlists' | 'devices' | 'activity' | 'settings'
const ORDER: Place[] = ['listen', 'music', 'health', 'inbox', 'playlists', 'devices', 'activity', 'settings']
/** The places on the top of the rail, in order (Ctrl+1 … Ctrl+6). */
const MAIN = 6

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
  const [busy, setBusy] = useState('')
  const [error, setError] = useState('')
  const [active, setActive] = useState<Place>('listen')
  const [direction, setDirection] = useState(1)
  const [backdrop, setBackdrop] = useState<string[]>([])
  const [deep, setDeep] = useState<{ done: number; total: number } | null>(null)
  const [lookup, setLookup] = useState<{ done: number; total: number; note?: string } | null>(null)
  const [notice, setNotice] = useState('')
  const [palette, setPalette] = useState(false)
  const [sheet, setSheet] = useState(false)
  const [inboxCount, setInboxCount] = useState(0)
  // The guided set-up: shown until a library is chosen, and on request.
  const [setup, setSetup] = useState(false)
  const search = useRef<HTMLInputElement>(null)
  const closeNotice = useCallback(() => setNotice(''), [])
  const closeError = useCallback(() => setError(''), [])

  const refresh = useCallback(async (dir: string, rescan: boolean): Promise<void> => {
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
      setBusy('')
      // Covers: a spread of albums for the background first, then everything for the wall.
      const dirs = r.albums.filter((a) => a.cover_track).map((a) => a.dir)
      await track({ id: 'covers', label: 'Making album covers', unit: 'albums' }, async (update) => {
        const step = Math.max(1, Math.floor(dirs.length / 24))
        const spread = dirs.filter((_, i) => i % step === 0).slice(0, 24)
        await loadCovers(dir, spread)
        const loaded = spread.map((d) => coverFor(d)).filter((c) => !!c)
        setBackdrop(loaded.map((c) => c.src))
        setAccent(loaded.find((c) => c.accent)?.accent)
        await loadCovers(dir, dirs, 320, update)
      })
    } catch (e) {
      setBusy('')
      // A folder that has gone (drive unplugged, renamed) gets its own screen, not an error.
      if (/does not exist or is not a directory/.test(String(e))) setMissing(dir)
      else setError(String(e))
    }
  }, [])

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
      const from = ORDER.indexOf(active)
      const to = ORDER.indexOf(id)
      if (to < 0 || to === from) return
      setDirection(to > from ? 1 : -1)
      setActive(id)
    },
    [active],
  )

  async function runDeepCheck() {
    setDeep({ done: 0, total: 0 })
    try {
      await track({ id: 'deep', label: 'Checking sound quality', unit: 'songs', cancel: () => void cancelDeepCheck() }, (update) =>
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
    }
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
    // Show the wall filling in rather than leaving you on the screen you chose it from.
    go('music')
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
      goTo('playlists', 'Playlists', 'Ctrl+5', 'smart want-list to get'),
      goTo('devices', 'Devices', 'Ctrl+6', 'cards sd players sync'),
      goTo('activity', 'Activity', 'Ctrl+H', 'history undo builds'),
      goTo('settings', 'Settings', 'Ctrl+,', 'preferences options keys'),
      { id: 'search', group: 'Actions', label: 'Search your music', keys: '/', run: () => focusSearch() },
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
        label: 'Accent colour from the album',
        keywords: 'color dynamic',
        run: () => setPrefs({ accentMode: 'album' }),
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
          go('music')
          window.setTimeout(() => {
            if (search.current) {
              const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set
              set?.call(search.current, a.title)
              search.current.dispatchEvent(new Event('input', { bubbles: true }))
            }
          }, 120)
        },
      })
    }
    return list
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [report, root, go, prefs, toggleTheme])

  function focusSearch() {
    go('music')
    window.setTimeout(() => search.current?.focus(), 80)
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
      } else if (e.ctrlKey && /^[1-6]$/.test(k)) {
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
    { id: 'playlists', label: 'Playlists', icon: IconPlaylists, keys: 'Ctrl+5' },
    { id: 'devices', label: 'Devices', icon: IconCards, keys: 'Ctrl+6' },
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
    switch (active) {
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
            onChooseFolder={() => void chooseRoot()}
            onOrganize={() => go('inbox')}
            running={{ lookup: !!lookup, check: !!deep }}
            progress={
              <>
                <AnimatePresence>
                  {deep && <DeepProgressBar deep={deep} label="Checking quality" unit="songs" onCancel={() => void cancelDeepCheck()} />}
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
            onImported={() => void refresh(root, false)}
            onCount={setInboxCount}
            onSwitchLibrary={(path) => {
              openLibrary(path)
              go('music')
              setNotice(`Now showing your new library in ${path}.`)
            }}
          />
        )
      case 'playlists':
        return <PlaylistsScreen root={root} />
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
        <Backdrop covers={backdrop} />
        <TitleBar onPalette={() => setPalette(true)} />
        <div className="relative z-10 flex min-h-0 flex-1">
          <Rail
            top={top}
            bottom={bottom}
            active={setup ? '' : active}
            onSelect={(id) => {
              setSetup(false)
              go(id as Place)
            }}
          />
          <main className="relative min-w-0 flex-1 overflow-hidden">
            <AnimatePresence mode="popLayout" custom={direction} initial={false}>
              <motion.div
                key={active}
                custom={direction}
                variants={{
                  enter: (d: number) => ({ x: d * 70, opacity: 0, scale: 0.985 }),
                  center: { x: 0, opacity: 1, scale: 1 },
                  exit: (d: number) => ({ x: d * -70, opacity: 0, scale: 0.985 }),
                }}
                initial="enter"
                animate="center"
                exit="exit"
                transition={{ type: 'spring', stiffness: 260, damping: 30 }}
                className="no-scrollbar absolute inset-0 overflow-y-auto px-10"
              >
                {screen}
              </motion.div>
            </AnimatePresence>
            <Toast text={error ? '' : notice} onClose={closeNotice} />
            <Toast text={error} tone="error" onClose={closeError} />
          </main>
        </div>
        <CommandPalette open={palette} onClose={() => setPalette(false)} commands={commands} />
        <ShortcutSheet open={sheet} onClose={() => setSheet(false)} />
      </div>
    </MotionConfig>
  )
}
