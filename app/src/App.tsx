import { AnimatePresence, MotionConfig, motion } from 'motion/react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { Backdrop } from './components/Backdrop'
import { Rail, type Section } from './components/Rail'
import { TitleBar } from './components/TitleBar'
import { CommandPalette, ShortcutSheet, type Command } from './components/CommandPalette'
import {
  IconActivity,
  IconCards,
  IconInbox,
  IconMusic,
  IconPlaylists,
  IconRefresh,
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
  libraryHealth,
  scanLibrary,
  type HealthReport,
} from './lib/engine'
import { coverFor, loadCovers, setAccent } from './lib/covers'
import { getPrefs, motionSetting, resolvedTheme, setPrefs, usePrefs, type Surface } from './lib/prefs'
import { DeepProgressBar, EmptyState, LibraryScreen, PillButton } from './screens/LibraryScreen'
import { MusicScreen } from './screens/MusicScreen'
import { InboxScreen } from './screens/InboxScreen'
import { CardsScreen } from './screens/CardsScreen'
import { PlaylistsScreen } from './screens/PlaylistsScreen'
import { ActivityScreen } from './screens/ActivityScreen'
import { SettingsScreen } from './screens/SettingsScreen'

const ROOT_KEY = 'library-root'
const store = {
  get: () => {
    try {
      return localStorage.getItem(ROOT_KEY) ?? ''
    } catch {
      return ''
    }
  },
  set: (v: string) => {
    try {
      localStorage.setItem(ROOT_KEY, v)
    } catch {
      // Not remembered this time; harmless.
    }
  },
}

type Place = 'music' | 'inbox' | 'playlists' | 'devices' | 'activity' | 'settings'
const ORDER: Place[] = ['music', 'inbox', 'playlists', 'devices', 'activity', 'settings']

export default function App() {
  const prefs = usePrefs()
  const [root, setRoot] = useState(() => store.get() || (inApp() ? '' : 'D:/Collection'))
  const [report, setReport] = useState<HealthReport | null>(null)
  const [busy, setBusy] = useState('')
  const [error, setError] = useState('')
  const [active, setActive] = useState<Place>('music')
  const [musicView, setMusicView] = useState<'albums' | 'quality'>('albums')
  const [direction, setDirection] = useState(1)
  const [backdrop, setBackdrop] = useState<string[]>([])
  const [deep, setDeep] = useState<{ done: number; total: number } | null>(null)
  const [lookup, setLookup] = useState<{ done: number; total: number } | null>(null)
  const [notice, setNotice] = useState('')
  const [palette, setPalette] = useState(false)
  const [sheet, setSheet] = useState(false)
  const [inboxCount, setInboxCount] = useState(0)
  const search = useRef<HTMLInputElement>(null)

  const refresh = useCallback(async (dir: string, rescan: boolean): Promise<void> => {
    try {
      if (rescan) {
        setBusy('Reading your music…')
        await scanLibrary(dir)
      }
      setBusy('Checking your files…')
      let r = await libraryHealth(dir)
      // A library that was never read shows as empty: read it once, then report.
      if (!rescan && r.summary.tracks === 0) {
        setBusy('Reading your music…')
        await scanLibrary(dir)
        r = await libraryHealth(dir)
      }
      setReport(r)
      setError('')
      setBusy('')
      // Covers: a spread of albums for the background first, then everything for the wall.
      const dirs = r.albums.filter((a) => a.cover_track).map((a) => a.dir)
      const step = Math.max(1, Math.floor(dirs.length / 24))
      const spread = dirs.filter((_, i) => i % step === 0).slice(0, 24)
      await loadCovers(dir, spread)
      const loaded = spread.map((d) => coverFor(d)).filter((c) => !!c)
      setBackdrop(loaded.map((c) => c.src))
      setAccent(loaded.find((c) => c.accent)?.accent)
      await loadCovers(dir, dirs)
    } catch (e) {
      setBusy('')
      setError(String(e))
    }
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
          const r = await syncCard(root, c.volume.root, () => {})
          await ejectCard(c.volume.root).catch(() => {})
          setNotice(`${c.card.name} is up to date (${r.copied} copied, ${r.patched} updated, ${r.deleted} removed) and safe to remove.`)
        } catch (e) {
          setError(String(e))
        }
      }
    }, 5000)
    return () => window.clearInterval(t)
  }, [root])

  // On launch, show the last library straight from its index.
  useEffect(() => {
    if (root) void refresh(root, false)
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
      await deepCheck(root, (done, total) => setDeep({ done, total }))
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
      const r = await lookupOnline(root, (done, total) => setLookup({ done, total }))
      setNotice(
        r.cancelled
          ? 'Stopped. What was found is kept; look up again to continue.'
          : `Looked up ${r.albums_looked_up} albums (${r.musicbrainz_matches} on MusicBrainz, ${r.deezer_matches} on Deezer, ${r.apple_matches} on Apple${r.discogs_matches ? `, ${r.discogs_matches} on Discogs` : ''}) and found lyrics for ${r.lyrics_found} of ${r.lyrics_checked} songs${r.identified ? `; recognised ${r.identified} files without tags by their sound` : ''}. The Inbox shows what changes.`,
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
    store.set(picked)
    setReport(null)
    setBackdrop([])
    setRoot(picked)
    refresh(picked, true)
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
      goTo('music', 'Music', 'Ctrl+1', 'albums library home'),
      goTo('inbox', 'Inbox', 'Ctrl+2', 'questions decisions review new music incoming'),
      goTo('playlists', 'Playlists', 'Ctrl+3', 'smart want-list to get'),
      goTo('devices', 'Devices', 'Ctrl+4', 'cards sd players sync'),
      goTo('activity', 'Activity', 'Ctrl+H', 'history undo builds'),
      goTo('settings', 'Settings', 'Ctrl+,', 'preferences options keys'),
      { id: 'search', group: 'Actions', label: 'Search your music', keys: '/', run: () => focusSearch() },
      {
        id: 'quality',
        group: 'Actions',
        label: 'Check quality',
        keywords: 'deep check integrity lossy upsampled loudness',
        run: () => {
          go('music')
          setMusicView('quality')
          void runDeepCheck()
        },
      },
      { id: 'lookup', group: 'Actions', label: 'Look up details online', keywords: 'musicbrainz covers lyrics credits years', run: () => void runLookup() },
      { id: 'rescan', group: 'Actions', label: 'Rescan your music', keywords: 'refresh reload', run: () => void refresh(root, true) },
      { id: 'choose', group: 'Actions', label: 'Choose the folder with your music', keywords: 'library open', run: () => void chooseRoot() },
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
          setMusicView('albums')
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
    setMusicView('albums')
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
      } else if (e.ctrlKey && /^[1-4]$/.test(k)) {
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
      const places = ORDER.slice(0, 4)
      const i = places.indexOf(active)
      const next = i >= 0 ? places[i + (s.acc > 0 ? 1 : -1)] : undefined
      s.acc = 0
      s.lock = now + 650
      if (next) go(next)
    }
  }

  const top: Section[] = [
    { id: 'music', label: 'Music', icon: IconMusic, keys: 'Ctrl+1' },
    { id: 'inbox', label: 'Inbox', icon: IconInbox, keys: 'Ctrl+2', badge: inboxCount },
    { id: 'playlists', label: 'Playlists', icon: IconPlaylists, keys: 'Ctrl+3' },
    { id: 'devices', label: 'Devices', icon: IconCards, keys: 'Ctrl+4' },
  ]
  const bottom: Section[] = [
    { id: 'activity', label: 'Activity', icon: IconActivity, keys: 'Ctrl+H' },
    { id: 'settings', label: 'Settings', icon: IconSettings, keys: 'Ctrl+,' },
  ]

  const screen = (() => {
    if (!root && active !== 'settings') return <EmptyState onChoose={chooseRoot} />
    switch (active) {
      case 'music':
        return (
          <MusicScreen
            root={root}
            report={report}
            busy={busy}
            view={musicView}
            onView={setMusicView}
            searchRef={search}
            actions={
              <>
                <PillButton onClick={runLookup} disabled={!!busy || !!lookup} title="Original years, credits, sharper covers and lyrics. Nothing changes until you approve it in the Inbox.">
                  Look up details online
                </PillButton>
                <PillButton onClick={runDeepCheck} disabled={!!busy || !!deep} title="Decode every new or changed song: integrity, quality and loudness. Changes nothing.">
                  Check quality
                </PillButton>
                <PillButton onClick={() => refresh(root, true)} disabled={!!busy} title="Read the folder again for new or changed files">
                  <IconRefresh />
                </PillButton>
              </>
            }
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
            quality={
              <LibraryScreen
                bare
                root={root}
                report={report}
                busy={busy}
                onChoose={chooseRoot}
                onRescan={() => refresh(root, true)}
                deep={deep}
                onDeepCheck={runDeepCheck}
                onCancelDeep={() => void cancelDeepCheck()}
                lookup={lookup}
                onLookup={runLookup}
                onCancelLookup={() => void cancelLookup()}
              />
            }
          />
        )
      case 'inbox':
        return <InboxScreen root={root} onImported={() => void refresh(root, false)} onCount={setInboxCount} />
      case 'playlists':
        return <PlaylistsScreen root={root} />
      case 'devices':
        return <CardsScreen root={root} />
      case 'activity':
        return <ActivityScreen root={root} />
      case 'settings':
        return <SettingsScreen root={root} onChooseLibrary={chooseRoot} />
    }
  })()

  return (
    <MotionConfig reducedMotion={motionSetting(prefs)}>
      <div className="relative flex h-full flex-col" onWheel={onWheel}>
        <Backdrop covers={backdrop} />
        <TitleBar onPalette={() => setPalette(true)} />
        <div className="relative z-10 flex min-h-0 flex-1">
          <Rail top={top} bottom={bottom} active={active} onSelect={(id) => go(id as Place)} />
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
            <AnimatePresence>
              {notice && !error && (
                <motion.div
                  initial={{ y: 40, opacity: 0 }}
                  animate={{ y: 0, opacity: 1 }}
                  exit={{ y: 40, opacity: 0 }}
                  onClick={() => setNotice('')}
                  className="popover absolute bottom-5 left-1/2 max-w-[680px] -translate-x-1/2 cursor-pointer rounded-2xl px-5 py-3 text-[13px] text-ink/85"
                >
                  {notice}
                </motion.div>
              )}
            </AnimatePresence>
            <AnimatePresence>
              {error && (
                <motion.div
                  initial={{ y: 40, opacity: 0 }}
                  animate={{ y: 0, opacity: 1 }}
                  exit={{ y: 40, opacity: 0 }}
                  onClick={() => setError('')}
                  className="popover absolute bottom-5 left-1/2 max-w-[640px] -translate-x-1/2 cursor-pointer rounded-2xl px-5 py-3 text-[13px]"
                >
                  <span className="text-problem">Something went wrong. </span>
                  <span className="text-ink/75">{error}</span>
                </motion.div>
              )}
            </AnimatePresence>
          </main>
        </div>
        <CommandPalette open={palette} onClose={() => setPalette(false)} commands={commands} />
        <ShortcutSheet open={sheet} onClose={() => setSheet(false)} />
      </div>
    </MotionConfig>
  )
}
