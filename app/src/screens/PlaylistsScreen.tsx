import { AnimatePresence, motion } from 'motion/react'
import { Suspense, lazy, useCallback, useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { useCatalog } from '../lib/catalog'
import {
  inApp,
  playlists as loadPlaylists,
  type HealthReport,
  type PlaylistsView,
} from '../lib/engine'
import { useEscape } from '../lib/layers'
import { playlistsChanged } from '../components/AddToPlaylist'
import type { ImportStart } from './playlists/ImportSheet'
import { PlaylistPage } from './playlists/PlaylistPage'
import { PlaylistsHome } from './playlists/PlaylistsHome'
import { WantList } from './playlists/WantList'

// Importing is opened now and then: it loads the first time.
const ImportSheet = lazy(() => import('./playlists/ImportSheet').then((m) => ({ default: m.ImportSheet })))

export type PlaylistsPlace =
  | { kind: 'home' }
  | { kind: 'playlist'; name: string; fresh?: boolean; rename?: boolean; draft?: boolean }
  | { kind: 'wanted' }
  | { kind: 'import'; start?: ImportStart }
  | { kind: 'new' }

/*
  Where Playlists should open next, asked from outside (the add-to-playlist picker's Open, the
  palette's Import playlists). The screen takes it when it shows, or at once if it is showing.
*/
let asked: PlaylistsPlace | null = null
const listeners = new Set<() => void>()
export function showPlaylists(place: PlaylistsPlace) {
  asked = place
  listeners.forEach((l) => l())
}
const subscribe = (l: () => void) => {
  listeners.add(l)
  return () => {
    listeners.delete(l)
  }
}

type View = Exclude<PlaylistsPlace, { kind: 'new' }> & { id: number }
let viewSeq = 0
const at = (p: Exclude<PlaylistsPlace, { kind: 'new' }>): View => ({ ...p, id: ++viewSeq })
// Where you were, kept while you visit other places.
let keptView: View = at({ kind: 'home' })
let keptRoot = ''
const kindListeners = new Set<() => void>()
const kept = {
  get view() {
    return keptView
  },
  set view(v: View) {
    keptView = v
    kindListeners.forEach((l) => l())
  },
}
/** What Playlists shows ("home", "playlist", "import", "wanted"), or is about to show. */
export function usePlaylistsKind() {
  return useSyncExternalStore(
    (l) => {
      kindListeners.add(l)
      listeners.add(l)
      return () => {
        kindListeners.delete(l)
        listeners.delete(l)
      }
    },
    () => (asked && asked.kind !== 'new' ? asked.kind : keptView.kind),
  )
}

export function PlaylistsScreen(props: { root: string; report: HealthReport | null; onAlbum: (dir: string) => void; embedded?: boolean }) {
  const { root } = props
  const [data, setData] = useState<PlaylistsView | null>(null)
  const [error, setError] = useState('')
  const [view, setViewState] = useState<View>(() => {
    // Opened to show something (a playlist from the Music home): start there, with no flash
    // of where you were before.
    if (asked && asked.kind !== 'new') {
      keptView = at(asked)
      asked = null
      return keptView
    }
    return keptRoot === root ? keptView : at({ kind: 'home' })
  })
  keptRoot = root
  const catalog = useCatalog(root, props.report)
  const box = useRef<HTMLDivElement>(null)
  const setView = (v: View) => {
    kept.view = v
    setViewState(v)
    box.current?.closest('.overflow-y-auto')?.scrollTo({ top: 0 })
  }

  const load = useCallback(async () => {
    try {
      setData(await loadPlaylists(root))
      setError('')
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }, [root])

  useEffect(() => {
    if (!root) return
    void load()
    const on = () => void load()
    window.addEventListener('venuta:playlists-changed', on)
    return () => window.removeEventListener('venuta:playlists-changed', on)
  }, [root, load])

  async function create() {
    const names = new Set((data?.playlists ?? []).map((p) => p.name.toLowerCase()))
    let name = 'New playlist'
    for (let n = 2; names.has(name.toLowerCase()); n++) name = `New playlist ${n}`
    // A draft: nothing is written until it has a name or a song, so leaving it untouched
    // leaves nothing behind.
    setView(at({ kind: 'playlist', name, fresh: true, draft: true }))
  }

  // Requests from outside.
  const request = useSyncExternalStore(subscribe, () => asked)
  useEffect(() => {
    if (!request || !root) return
    asked = null
    if (request.kind === 'new') void create()
    else setView(at(request))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [request, root])

  const home = () => setView(at({ kind: 'home' }))
  useEscape(view.kind !== 'home', home)

  const ruleOf = (name: string) => data?.smart.find((r) => r.name === name)



  if (!root) return <div className="grid h-full place-items-center text-ink/60">Choose a library first.</div>
  if (!inApp()) return <div className="grid h-full place-items-center text-ink/60">Playlists are managed by the desktop app.</div>

  return (
    <div ref={box}>
      <AnimatePresence mode="wait" initial={false}>
        <motion.div
          key={view.kind === 'playlist' || view.kind === 'import' ? `${view.kind}${view.id}` : view.kind}
          initial={{ opacity: 0, x: view.kind === 'home' ? -14 : 14 }}
          animate={{ opacity: 1, x: 0 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.18, ease: 'easeOut' }}
        >
          {view.kind === 'home' && (
            <PlaylistsHome
              root={root}
              data={data}
              error={error}
              onOpen={(name) => setView(at({ kind: 'playlist', name }))}
              onOpenWanted={() => setView(at({ kind: 'wanted' }))}
              onNew={() => void create()}
              onImport={() => setView(at({ kind: 'import' }))}
              onComplete={(only) => setView(at({ kind: 'import', start: { kind: 'apple', only } }))}
              embedded={props.embedded}
            />
          )}
          {view.kind === 'playlist' && (
            <PlaylistPage
              root={root}
              name={view.name}
              summary={data?.playlists.find((p) => p.name === view.name)}
              catalog={catalog}
              rule={ruleOf(view.name)}
              fresh={view.fresh}
              draft={view.draft}
              rename={view.rename}
              onBack={home}
              onChanged={() => void load()}
              onRenamed={(name) => {
                // Same page, new name: keep its id so nothing reloads or animates.
                const next = { ...view, name, fresh: view.fresh, rename: false, draft: false }
                kept.view = next
                setViewState(next)
                void load()
                playlistsChanged()
              }}
              onGone={() => {
                void load()
                home()
              }}
              onOpen={(name) => setView(at({ kind: 'playlist', name }))}
              onAlbum={props.onAlbum}
              onUpdate={(name) => setView(at({ kind: 'import', start: { kind: 'update', name } }))}
            />
          )}
          {view.kind === 'wanted' && <WantList root={root} wanted={data?.wanted ?? []} onBack={home} onOpen={(name) => setView(at({ kind: 'playlist', name }))} onChanged={() => void load()} />}
          {view.kind === 'import' && (
            <Suspense fallback={null}>
              <ImportSheet
                root={root}
                appleReady={!!data?.apple_ready}
                start={view.start}
                onBack={home}
                onOpen={(name) => setView(at({ kind: 'playlist', name }))}
                onOpenWanted={() => setView(at({ kind: 'wanted' }))}
                onChanged={() => void load()}
              />
            </Suspense>
          )}
        </motion.div>
      </AnimatePresence>
    </div>
  )
}
