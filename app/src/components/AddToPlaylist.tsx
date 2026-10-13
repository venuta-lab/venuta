import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react'
import { playlistAdd, playlists as loadPlaylists, type PlaylistSummary } from '../lib/engine'
import { useEscape } from '../lib/layers'
import { notify } from '../lib/notify'
import { Mosaic } from './Mosaic'

/*
  "Add to playlist…" from anywhere (a song's or an album's menu, Now Playing, the queue): one
  picker for the whole app. Any code asks with askAddToPlaylist(); <AddToPlaylist> shows it.
*/
type Request = { paths: string[]; label: string }
let request: Request | null = null
const listeners = new Set<() => void>()
const emit = () => listeners.forEach((l) => l())

/** Open the picker for these songs; `label` names them ("Tum Hi Ho", "Aashiqui 2"). */
export function askAddToPlaylist(paths: string[], label: string) {
  if (!paths.length) return
  request = { paths, label }
  emit()
}

/** Tell open playlist views that a playlist changed (so they re-read it). */
export const playlistsChanged = () => window.dispatchEvent(new CustomEvent('venuta:playlists-changed'))

export function AddToPlaylist({ root, onOpen }: { root: string; onOpen: (name: string) => void }) {
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
    const s = q.trim().toLowerCase()
    return (lists ?? []).filter((p) => !s || p.name.toLowerCase().includes(s))
  }, [lists, q])
  const exact = (lists ?? []).some((p) => p.name.toLowerCase() === q.trim().toLowerCase())

  async function add(name: string) {
    if (!req || busy) return
    setBusy(true)
    try {
      const n = await playlistAdd(root, name, req.paths)
      const what = req.paths.length === 1 ? req.label : `${req.paths.length} songs`
      notify(
        n === 0 ? `${what} ${req.paths.length === 1 ? 'is' : 'are'} already in ${name}.` : `Added ${n === req.paths.length ? what : `${n} of ${req.paths.length} songs`} to ${name}.`,
        { label: 'Open', run: () => onOpen(name) },
      )
      playlistsChanged()
      close()
    } catch (e) {
      notify(`Couldn't add to ${name}: ${e instanceof Error ? e.message : String(e)}`)
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
                Add {req.paths.length === 1 ? `“${req.label}”` : `${req.paths.length} songs from ${req.label}`} to
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
                    else if (q.trim()) void add(q.trim())
                  }
                }}
                placeholder="Find a playlist, or name a new one"
                className="mt-1.5 w-full bg-transparent text-[16px] text-ink outline-none placeholder:text-ink/40"
              />
            </div>
            <div className="fade-y min-h-0 flex-1 overflow-y-auto p-1.5">
              {q.trim() && !exact && (
                <button
                  onClick={() => void add(q.trim())}
                  className="flex w-full items-center gap-3 rounded-xl px-2.5 py-2 text-left transition-colors hover:bg-ink/[0.07] active:bg-ink/[0.1]"
                >
                  <span className="grid h-10 w-10 shrink-0 place-items-center rounded-lg bg-accent text-[20px] leading-none text-on-accent">+</span>
                  <span className="min-w-0">
                    <span className="block truncate text-[14px] font-medium">New playlist “{q.trim()}”</span>
                    <span className="block text-[12px] text-ink/55">Made with {req.paths.length === 1 ? 'this song' : `these ${req.paths.length} songs`}</span>
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
                  <Mosaic root={root} dirs={p.dirs} className="h-10 w-10 shrink-0 rounded-lg" />
                  <span className="min-w-0">
                    <span className="block truncate text-[14px]">{p.name}</span>
                    <span className="block text-[12px] text-ink/50">
                      {p.tracks - p.missing} {p.tracks - p.missing === 1 ? 'song' : 'songs'}
                    </span>
                  </span>
                </button>
              ))}
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}
