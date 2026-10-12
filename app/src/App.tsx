import { AnimatePresence, motion } from 'motion/react'
import { useCallback, useEffect, useRef, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { Backdrop } from './components/Backdrop'
import { Rail, type Section } from './components/Rail'
import { TitleBar } from './components/TitleBar'
import {
  IconBrowse,
  IconCards,
  IconHealth,
  IconIncoming,
  IconPlaylists,
  IconReview,
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
import { LibraryScreen } from './screens/LibraryScreen'
import { BrowseScreen } from './screens/BrowseScreen'
import { SoonScreen } from './screens/SoonScreen'
import { ReviewScreen } from './screens/ReviewScreen'
import { IncomingScreen } from './screens/IncomingScreen'
import { CardsScreen } from './screens/CardsScreen'
import { PlaylistsScreen } from './screens/PlaylistsScreen'

const SECTIONS: Section[] = [
  { id: 'library', label: 'Library', icon: IconHealth, ready: true },
  { id: 'browse', label: 'Browse', icon: IconBrowse, ready: true },
  { id: 'review', label: 'Review', icon: IconReview, ready: true },
  { id: 'incoming', label: 'Incoming', icon: IconIncoming, ready: true },
  { id: 'playlists', label: 'Playlists', icon: IconPlaylists, ready: true },
  { id: 'cards', label: 'Cards', icon: IconCards, ready: true },
]

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

export default function App() {
  const [root, setRoot] = useState(() => store.get() || (inApp() ? '' : 'D:/Collection'))
  const [report, setReport] = useState<HealthReport | null>(null)
  const [busy, setBusy] = useState('')
  const [error, setError] = useState('')
  const [active, setActive] = useState('library')
  const [direction, setDirection] = useState(1)
  const [backdrop, setBackdrop] = useState<string[]>([])
  const [deep, setDeep] = useState<{ done: number; total: number } | null>(null)
  const [lookup, setLookup] = useState<{ done: number; total: number } | null>(null)
  const [notice, setNotice] = useState('')

  const refresh = useCallback(async (dir: string, rescan: boolean): Promise<void> => {
    try {
      if (rescan) {
        setBusy('Scanning your library…')
        await scanLibrary(dir)
      }
      setBusy('Checking library health…')
      let r = await libraryHealth(dir)
      // A library that was never indexed reads as empty: scan it once, then report.
      if (!rescan && r.summary.tracks === 0) {
        setBusy('Scanning your library…')
        await scanLibrary(dir)
        r = await libraryHealth(dir)
      }
      setReport(r)
      setError('')
      setBusy('')
      // Covers: a spread of albums for the backdrop first, then everything for the wall.
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
    (id: string) => {
      const from = SECTIONS.findIndex((s) => s.id === active)
      const to = SECTIONS.findIndex((s) => s.id === id)
      if (to < 0 || to === from) return
      setDirection(to > from ? 1 : -1)
      setActive(id)
    },
    [active],
  )

  // Ctrl+1..6 jumps between sections.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.ctrlKey) return
      const n = Number(e.key)
      if (n >= 1 && n <= SECTIONS.length) {
        e.preventDefault()
        go(SECTIONS[n - 1].id)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [go])

  // Two-finger horizontal swipe on a trackpad moves to the next/previous section.
  const swipe = useRef({ acc: 0, lock: 0 })
  const onWheel = (e: React.WheelEvent) => {
    if (Math.abs(e.deltaX) < Math.abs(e.deltaY) * 1.5) return
    const s = swipe.current
    const now = performance.now()
    if (now < s.lock) return
    s.acc += e.deltaX
    if (Math.abs(s.acc) > 160) {
      const i = SECTIONS.findIndex((x) => x.id === active)
      const next = SECTIONS[i + (s.acc > 0 ? 1 : -1)]
      s.acc = 0
      s.lock = now + 650
      if (next) go(next.id)
    }
  }

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
          ? 'Lookup stopped. What was found is kept; run it again to continue.'
          : `Looked up ${r.albums_looked_up} albums (${r.musicbrainz_matches} on MusicBrainz, ${r.deezer_matches} on Deezer, ${r.apple_matches} covers from Apple) and found lyrics for ${r.lyrics_found} of ${r.lyrics_checked} tracks. Open Review to see what changes.`,
      )
    } catch (e) {
      setError(String(e))
    } finally {
      setLookup(null)
    }
  }

  async function chooseRoot() {
    const picked = inApp()
      ? await open({ directory: true, title: 'Choose your music library folder' })
      : null
    if (typeof picked !== 'string') return
    store.set(picked)
    setReport(null)
    setBackdrop([])
    setRoot(picked)
    refresh(picked, true)
  }

  const screen = (() => {
    switch (active) {
      case 'library':
        return (
          <LibraryScreen
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
        )
      case 'browse':
        return <BrowseScreen root={root} albums={report?.albums ?? []} />
      case 'review':
        return <ReviewScreen root={root} />
      case 'playlists':
        return <PlaylistsScreen root={root} />
      case 'cards':
        return <CardsScreen root={root} />
      case 'incoming':
        return <IncomingScreen root={root} onImported={() => void refresh(root, false)} />
      default:
        return <SoonScreen section={SECTIONS.find((s) => s.id === active)!} />
    }
  })()

  return (
    <div className="relative flex h-full flex-col" onWheel={onWheel}>
      <Backdrop covers={backdrop} />
      <TitleBar />
      <div className="relative z-10 flex min-h-0 flex-1">
        <Rail sections={SECTIONS} active={active} onSelect={go} />
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
                className="glass absolute bottom-5 left-1/2 max-w-[680px] -translate-x-1/2 cursor-pointer rounded-2xl px-5 py-3 text-[13px] text-ink/85"
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
                className="glass absolute bottom-5 left-1/2 max-w-[640px] -translate-x-1/2 rounded-2xl px-5 py-3 text-[13px]"
              >
                <span className="text-problem">Couldn’t read the library. </span>
                <span className="text-ink/75">{error}</span>
              </motion.div>
            )}
          </AnimatePresence>
        </main>
      </div>
    </div>
  )
}
