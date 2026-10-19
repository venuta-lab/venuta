import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react'
import { playlists as loadPlaylists, ytmPlaylistEdit, type OnlineCollection, type OnlineSuggestion, type PlaylistItem, type PlaylistSummary } from '../lib/engine'
import { useYtmPart, useYtmStatus } from '../lib/ytmLibrary'
import { notify } from '../lib/notify'
import { onlineProblem } from '../lib/online'
import { RemoteArt } from './RemoteArt'
import { useEscape } from '../lib/layers'
import { addSongsTo } from '../screens/playlists/ops'
import { nameNote, savedName } from '../lib/playlistName'
import { Mosaic } from './Mosaic'
import { keyFor, matcher } from '../lib/textsearch'

/*
  "Add to playlist…" from anywhere (a song's or an album's menu, Now Playing, the queue): one
  picker for the whole app. Any code asks with askAddToPlaylist(); <AddToPlaylist> shows it.
*/
type Request = { items: PlaylistItem[]; label: string }
let request: Request | null = null
const listeners = new Set<() => void>()
const emit = () => listeners.forEach((l) => l())

/** Open the picker for these library songs; `label` names them ("Tum Hi Ho", "Aashiqui 2"). */
export const askAddToPlaylist = (paths: string[], label: string) =>
  askAddSongs(
    paths.filter(Boolean).map((path) => ({ path })),
    label,
  )

/** Open the picker for any songs: library files, and online songs by their stable id. */
export function askAddSongs(items: PlaylistItem[], label: string) {
  if (!items.length) return
  request = { items, label }
  emit()
}

/** Tell open playlist views that a playlist changed (so they re-read it). */
export const playlistsChanged = () => window.dispatchEvent(new CustomEvent('venuta:playlists-changed'))

export function AddToPlaylist({ root }: { root: string }) {
  const req = useSyncExternalStore(
    (l) => {
      listeners.add(l)
      return () => {
        listeners.delete(l)
      }
    },
    () => request,
  )
  const close = () => {
    request = null
    emit()
  }
  useEscape(!!req, close)
  const [lists, setLists] = useState<PlaylistSummary[] | null>(null)
  const [q, setQ] = useState('')
  const [busy, setBusy] = useState(false)
  const input = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (!req) return
    setQ('')
    setLists(null)
    loadPlaylists(root)
      .then((v) => setLists(v.playlists.filter((p) => !p.smart)))
      .catch(() => setLists([]))
    window.setTimeout(() => input.current?.focus(), 30)
  }, [req, root])

  const shown = useMemo(() => {
    const has = matcher(q)
    return (lists ?? []).filter((p) => has(keyFor(p, () => p.name)))
  }, [lists, q])
  // A new playlist is made under the name it will be saved as (lib/playlistName.ts).
  const fresh = savedName(q)
  const freshNote = nameNote(q)
  const exact = (lists ?? []).some((p) => p.name.toLowerCase() === fresh.toLowerCase())

  // Your YouTube Music playlists take the online songs among these (your own files stay in Venuta).
  const { signed_in } = useYtmStatus()
  const ytmLists = useYtmPart<OnlineSuggestion[]>('playlists', !!req).value
  const videoIds = (req?.items ?? []).flatMap((i) => ('online' in i && i.online.provider === 'remote_catalog' ? [i.online.track_id] : []))
  const onlyHere = (req?.items.length ?? 0) - videoIds.length
  const ytmShown = useMemo(() => {
    const has = matcher(q)
    return (
      (ytmLists ?? [])
        .flatMap((c) => (c.kind === 'playlist' ? [c.collection] : []))
        // Liked Music fills itself from likes; it isn't added to.
        .filter((c) => !/^(VL)?LM$/.test(c.collection_id) && has(keyFor({ name: c.title } as PlaylistSummary, () => c.title)))
    )
  }, [ytmLists, q])
  async function addToYtm(c: OnlineCollection) {
    if (!req || busy || !videoIds.length) return
    setBusy(true)
    try {
      await ytmPlaylistEdit(c.collection_id.replace(/^VL/, ''), [{ kind: 'add', video_ids: videoIds }])
      const what = videoIds.length === 1 ? `“${req.label}”` : `${videoIds.length} songs`
      const kept = onlyHere ? ` ${onlyHere === 1 ? 'One is a file of yours and stays' : `${onlyHere} are files of yours and stay`} in Venuta.` : ''
      notify(`Added ${what} to “${c.title}” on YouTube Music.${kept}`)
      close()
    } catch (e) {
      notify(onlineProblem(e))
    } finally {
      setBusy(false)
    }
  }

  async function add(name: string) {
    if (!req || busy) return
    setBusy(true)
    try {
      // The same as dropping songs on a playlist (screens/playlists/ops.ts).
      await addSongsTo(root, name, req.items, req.label)
      close()
    } finally {
      setBusy(false)
    }
  }


  return (
    <AnimatePresence>
      {req && (
        <motion.div
          className="fixed inset-0 z-50 flex items-start justify-center bg-[var(--scrim)] pt-[14vh]"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.12 }}
          onMouseDown={close}
        >
          <motion.div
            role="dialog"
            aria-label="Add to playlist"
            className="popover flex max-h-[64vh] w-[min(440px,92vw)] flex-col overflow-hidden rounded-[18px]"
            initial={{ y: -10, scale: 0.98, opacity: 0 }}
            animate={{ y: 0, scale: 1, opacity: 1 }}
            exit={{ y: -6, scale: 0.98, opacity: 0 }}
            transition={{ type: 'spring', stiffness: 520, damping: 38 }}
            onMouseDown={(e) => e.stopPropagation()}
          >
            <div className="border-b border-edge px-4 pb-3 pt-4">
              <div className="truncate text-[12.5px] text-ink/55">
                Add {req.items.length === 1 ? `“${req.label}”` : `${req.items.length} songs from ${req.label}`} to
              </div>
              <input
                ref={input}
                value={q}
                onChange={(e) => setQ(e.target.value)}
                onKeyDown={(e) => {
                  // Enter takes the first playlist that matches, or makes a new one when none does.
                  if (e.key === 'Enter') {
                    e.preventDefault()
                    if (shown[0]) void add(shown[0].name)
                    else if (fresh) void add(fresh)
                  }
                }}
                placeholder="Find a playlist, or name a new one"
                className="mt-1.5 w-full bg-transparent text-[16px] text-ink outline-none placeholder:text-ink/40"
              />
            </div>
            <div className="fade-y min-h-0 flex-1 overflow-y-auto p-1.5">
              {fresh && !exact && (
                <button
                  onClick={() => void add(fresh)}
                  className="flex w-full items-center gap-3 rounded-xl px-2.5 py-2 text-left transition-colors hover:bg-ink/[0.07] active:bg-ink/[0.1]"
                >
                  <span className="grid h-10 w-10 shrink-0 place-items-center accent-fill rounded-lg text-[20px] leading-none text-on-accent">+</span>
                  <span className="min-w-0">
                    <span className="block truncate text-[14px] font-medium">New playlist “{fresh}”</span>
                    <span className="block text-[12px] text-ink/55">
                      {freshNote ?? `Made with ${req.items.length === 1 ? 'this song' : `these ${req.items.length} songs`}`}
                    </span>
                  </span>
                </button>
              )}
              {lists === null && <p className="px-3 py-6 text-center text-[13px] text-ink/50">Reading your playlists…</p>}
              {lists?.length === 0 && !q.trim() && (
                <p className="px-3 py-6 text-center text-[13px] leading-relaxed text-ink/55">No playlists yet. Type a name above to make one.</p>
              )}
              {shown.map((p) => (
                <button
                  key={p.name}
                  onClick={() => void add(p.name)}
                  className="flex w-full items-center gap-3 rounded-xl px-2.5 py-2 text-left transition-colors hover:bg-ink/[0.07] active:bg-ink/[0.1]"
                >
                  <Mosaic root={root} dirs={p.dirs} liked={p.liked} className="h-10 w-10 shrink-0 rounded-lg" />
                  <span className="min-w-0">
                    <span className="block truncate text-[14px]">{p.name}</span>
                    <span className="block truncate text-[12px] text-ink/50">
                      {(() => {
                        const local = Math.max(0, p.tracks - p.missing - (p.online ?? 0))
                        const online = p.online ?? 0
                        const toGet = p.missing
                        if (online || toGet) {
                          return [local ? `${local} local` : '', online ? `${online} online` : '', toGet ? `${toGet} to get` : '']
                            .filter(Boolean)
                            .join(' · ')
                        }
                        return `${local} ${local === 1 ? 'song' : 'songs'}`
                      })()}
                    </span>
                  </span>
                </button>
              ))}
              {signed_in && videoIds.length > 0 && ytmShown.length > 0 && (
                <>
                  <div className="px-2.5 pb-1 pt-3 text-[12px] font-medium text-ink/50">On YouTube Music</div>
                  {ytmShown.map((c) => (
                    <button
                      key={c.collection_id}
                      onClick={() => void addToYtm(c)}
                      className="flex w-full items-center gap-3 rounded-xl px-2.5 py-2 text-left transition-colors hover:bg-ink/[0.07] active:bg-ink/[0.1]"
                    >
                      <span className="h-10 w-10 shrink-0 overflow-hidden rounded-lg bg-ink/[0.08]">
                        <RemoteArt url={c.artwork_url} seed={c.title} size={40} imgClassName="h-full w-full object-cover" />
                      </span>
                      <span className="min-w-0">
                        <span className="block truncate text-[14px]">{c.title}</span>
                        <span className="block truncate text-[12px] text-ink/50">{c.subtitle ?? 'YouTube Music'}</span>
                      </span>
                    </button>
                  ))}
                </>
              )}
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}
