import { AnimatePresence, motion } from 'motion/react'
import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { save as saveDialog } from '@tauri-apps/plugin-dialog'
import { useCatalog } from '../lib/catalog'
import {
  inApp,
  playlistDelete,
  playlistDuplicate,
  playlistExport,
  playlists as loadPlaylists,
  playlistSaveItems,
  type HealthReport,
  type PlaylistSummary,
  type PlaylistsView,
} from '../lib/engine'
import { useEscape } from '../lib/layers'
import { notify } from '../lib/notify'
import { confirmAsk } from '../components/Confirm'
import { playlistsChanged } from '../components/AddToPlaylist'
import { playableSongs, plural } from './playlists/common'
import { ImportSheet, type ImportStart } from './playlists/ImportSheet'
import { PlaylistPage } from './playlists/PlaylistPage'
import { PlaylistsHome, type PlaylistMenu } from './playlists/PlaylistsHome'
import { WantList } from './playlists/WantList'

export type PlaylistsPlace =
  | { kind: 'home' }
  | { kind: 'playlist'; name: string; fresh?: boolean; rename?: boolean }
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
    try {
      await playlistSaveItems(root, name, [])
      await load()
      setView(at({ kind: 'playlist', name, fresh: true }))
    } catch (e) {
      notify(`Couldn't make a playlist: ${e instanceof Error ? e.message : String(e)}`)
    }
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

  /** A smart playlist's file may not be written yet; reading its songs writes it. */
  const ready = async (p: PlaylistSummary) => {
    const rule = ruleOf(p.name)
    if (rule) await playableSongs(root, p.name, rule)
  }

  const menu: PlaylistMenu = {
    rename: (p) => setView(at({ kind: 'playlist', name: p.name, rename: true })),
    duplicate: async (p) => {
      try {
        await ready(p)
        const made = await playlistDuplicate(root, p.name)
        await load()
        notify(`Made “${made}”, a copy you can change.`, { label: 'Open', run: () => setView(at({ kind: 'playlist', name: made })) })
      } catch (e) {
        notify(`Couldn't copy ${p.name}: ${e instanceof Error ? e.message : String(e)}`)
      }
    },
    exportFile: async (p) => {
      const to = await saveDialog({ defaultPath: `${p.name}.m3u8`, filters: [{ name: 'Playlist', extensions: ['m3u8'] }] })
      if (!to) return
      try {
        await ready(p)
        const n = await playlistExport(root, p.name, to)
        notify(`Saved ${plural(n, 'song')} to ${to}.`)
      } catch (e) {
        notify(`Couldn't export ${p.name}: ${e instanceof Error ? e.message : String(e)}`)
      }
    },
    remove: async (p) => {
      const ok = await confirmAsk({
        title: `Delete “${p.name}”?`,
        text: 'The playlist file is moved to the Set aside folder beside your music, not destroyed. Your songs stay where they are.',
        confirm: 'Delete playlist',
        danger: true,
      })
      if (!ok) return
      try {
        await playlistDelete(root, p.name)
        await load()
        notify(`Deleted ${p.name}. Its file is in Set aside.`)
      } catch (e) {
        notify(`Couldn't delete ${p.name}: ${e instanceof Error ? e.message : String(e)}`)
      }
    },
  }

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
              menu={menu}
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
              rename={view.rename}
              onBack={home}
              onChanged={() => void load()}
              onRenamed={(name) => {
                // Same page, new name: keep its id so nothing reloads or animates.
                const next = { ...view, name, fresh: view.fresh, rename: false }
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
            <ImportSheet
              root={root}
              appleReady={!!data?.apple_ready}
              start={view.start}
              onBack={home}
              onOpen={(name) => setView(at({ kind: 'playlist', name }))}
              onOpenWanted={() => setView(at({ kind: 'wanted' }))}
              onChanged={() => void load()}
            />
          )}
        </motion.div>
      </AnimatePresence>
    </div>
  )
}
