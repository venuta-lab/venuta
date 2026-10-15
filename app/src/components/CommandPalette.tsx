import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { IconSearch } from './Icons'
import { AvailabilityBadge } from './AvailabilityBadge'
import { onlineSearch, type OnlineTrack } from '../lib/engine'
import { OFFLINE, onlineProblem, useNetworkOnline } from '../lib/online'
import { enqueueItems, playItems, songItemOf, toRemoteItem } from '../lib/player'
import { menuFor } from '../lib/actions'
import { menuFromEvent, openMenu, type MenuGroups } from '../lib/menu'
import { askAddSongs } from './AddToPlaylist'
import { EXACTLY, keyFor, matcher, searchKey } from '../lib/textsearch'

export interface Command {
  id: string
  label: string
  /** Group heading: Go to, Actions, Look and feel, Albums… */
  group: string
  /** Extra words that should find it ("dark" finds "Switch to dark"). */
  keywords?: string
  /** Shortcut, shown so it can be learned. */
  keys?: string
  /** A second line, e.g. an album's artist. */
  detail?: string
  run: () => void
  /** Everything else that can be done with it (lib/actions.ts): right-click, or → when chosen. */
  menu?: () => MenuGroups
}

/**
 * Every typed word must be there (in any script, spelled loosely, a longer word off by a
 * letter); earlier and whole-word hits rank higher.
 */
function score(c: Command, words: string[], has: (key: string) => number): number {
  if (!words.length) return 1
  const hay = keyFor(c, () => `${c.label} ${c.keywords ?? ''} ${c.detail ?? ''} ${c.group}`)
  const how = has(hay)
  if (!how) return 0
  // A result with a slip in it comes after the exact ones.
  let s = how === EXACTLY ? 100 : 1
  for (const w of words) {
    const i = hay.indexOf(w)
    if (i >= 0) s += 10 - Math.min(i, 9) + (hay[i - 1] === ' ' || i === 0 ? 5 : 0)
  }
  if (searchKey(c.label).startsWith(words.join(' '))) s += 20
  return s
}

export function CommandPalette(props: {
  open: boolean
  onClose: () => void
  commands: Command[]
  /** Library root: when set, typed words also search the online catalog. */
  root?: string
}) {
  const [q, setQ] = useState('')
  const [sel, setSel] = useState(0)
  const [online, setOnline] = useState<{ q: string; tracks: OnlineTrack[]; error: string; loading: boolean }>({
    q: '',
    tracks: [],
    error: '',
    loading: false,
  })
  const input = useRef<HTMLInputElement>(null)
  const list = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (props.open) {
      setQ('')
      setSel(0)
      setOnline({ q: '', tracks: [], error: '', loading: false })
      window.setTimeout(() => input.current?.focus(), 0)
    }
  }, [props.open])

  // Online hits for the same words as the palette (debounced); none asked for while offline.
  const connected = useNetworkOnline()
  useEffect(() => {
    if (!props.open || !props.root) return
    const query = q.trim()
    // Too short to ask, or offline (said once, under the results).
    if (query.length < 2 || !connected) {
      setOnline({ q: query, tracks: [], error: query.length < 2 ? '' : OFFLINE, loading: false })
      return
    }
    setOnline((s) => ({ ...s, q: query, loading: true, error: '' }))
    let live = true
    const t = window.setTimeout(() => {
      onlineSearch(query)
        .then((r) => live && setOnline({ q: query, tracks: r.tracks.slice(0, 8), error: '', loading: false }))
        .catch((e) => live && setOnline({ q: query, tracks: [], error: onlineProblem(e), loading: false }))
    }, 320)
    return () => {
      live = false
      window.clearTimeout(t)
    }
  }, [props.open, props.root, q, connected])

  const results = useMemo(() => {
    const words = searchKey(q).split(' ').filter(Boolean)
    const has = matcher(q)
    const ranked = props.commands
      .map((c) => ({ c, s: score(c, words, has) }))
      .filter((x) => x.s > 0)
    // Without a query, show actions first and keep albums out of the way.
    const local = q.trim()
      ? ranked.sort((a, b) => b.s - a.s).slice(0, 32)
      : ranked.filter((x) => x.c.group !== 'Albums')

    const onlineCmds: { c: Command; s: number }[] =
      props.root && online.q === q.trim() && online.tracks.length
        ? online.tracks.map((t, i) => {
            const artists = t.artists.join(', ')
            return {
              s: 50 - i,
              c: {
                id: `online:${t.provider}:${t.track_id}`,
                group: 'Online',
                label: t.title,
                detail: [artists, t.album].filter(Boolean).join(' · '),
                keywords: `online stream ${artists}`,
                run: () => {
                  if (!props.root) return
                  void playItems(
                    props.root,
                    online.tracks.map(toRemoteItem),
                    i,
                    { kind: 'search', ref: q.trim(), label: `Online: ${q.trim()}` },
                  )
                },
                menu: () => {
                  const root = props.root ?? ''
                  const item = toRemoteItem(t)
                  return menuFor(
                    'song',
                    { online: true, noArtist: true },
                    {
                      play: () => void playItems(root, [item], 0, { kind: 'search', ref: q.trim(), label: `Online: ${q.trim()}` }),
                      next: () => void enqueueItems(root, [item], true),
                      queue: () => void enqueueItems(root, [item], false),
                      playlist: () => askAddSongs([songItemOf(item)], t.title),
                    },
                  )
                },
              } satisfies Command,
            }
          })
        : []

    const shown = [...local, ...onlineCmds]
    // Show a group heading above the first command of each group.
    return shown.map((x, i) => ({ c: x.c, head: i === 0 || shown[i - 1].c.group !== x.c.group ? x.c.group : '' }))
  }, [props.commands, props.root, q, online])

  useEffect(() => {
    list.current?.querySelector(`[data-i="${sel}"]`)?.scrollIntoView({ block: 'nearest' })
  }, [sel])

  function run(c: Command | undefined) {
    if (!c) return
    props.onClose()
    window.setTimeout(c.run, 0)
  }

  /** A result's menu, whose items close the palette first and then do their work. */
  const menuOf = (c: Command): MenuGroups =>
    (c.menu?.() ?? []).map((g) =>
      g.map((item) => ({
        ...item,
        run: () => {
          props.onClose()
          window.setTimeout(item.run, 0)
        },
      })),
    )

  function onKey(e: React.KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setSel((s) => Math.min(s + 1, results.length - 1))
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setSel((s) => Math.max(s - 1, 0))
    } else if (e.key === 'ArrowRight' && results[sel]?.c.menu && (e.currentTarget as HTMLInputElement).selectionStart === q.length) {
      // At the end of what was typed, → opens the chosen result's menu.
      e.preventDefault()
      const row = list.current?.querySelector<HTMLElement>(`[data-i="${sel}"]`)
      if (row) openMenu(menuOf(results[sel].c), { anchor: row }, e.currentTarget as HTMLElement)
    } else if (e.key === 'Enter') {
      e.preventDefault()
      run(results[sel]?.c)
    } else if (e.key === 'Escape') {
      e.preventDefault()
      props.onClose()
    }
  }

  return (
    <AnimatePresence>
      {props.open && (
        <motion.div
          className="fixed inset-0 z-50 flex items-start justify-center bg-[var(--scrim)] pt-[12vh]"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.12 }}
          onMouseDown={props.onClose}
        >
          <motion.div
            role="dialog"
            aria-label="Command palette"
            className="popover w-[min(640px,92vw)] overflow-hidden rounded-[20px]"
            initial={{ y: -12, scale: 0.98, opacity: 0 }}
            animate={{ y: 0, scale: 1, opacity: 1 }}
            exit={{ y: -8, scale: 0.98, opacity: 0 }}
            transition={{ type: 'spring', stiffness: 520, damping: 38 }}
            onMouseDown={(e) => e.stopPropagation()}
          >
            <div className="flex items-center gap-3 border-b border-edge px-5 py-4">
              <IconSearch className="text-ink/50" />
              <input
                ref={input}
                id="palette-input"
                value={q}
                onChange={(e) => {
                  setQ(e.target.value)
                  setSel(0)
                }}
                onKeyDown={onKey}
                placeholder={props.root ? 'Type a command, album, artist — or an online song' : 'Type a command, an album or an artist'}
                className="min-w-0 flex-1 bg-transparent text-[16px] text-ink outline-none placeholder:text-ink/40"
                role="combobox"
                aria-expanded="true"
                aria-controls="palette-list"
              />
              {online.loading && q.trim().length >= 2 && (
                <span className="spin h-3.5 w-3.5 shrink-0 rounded-full border-2 border-ink/20 border-t-ink/60" aria-label="Searching online" />
              )}
              <kbd>Esc</kbd>
            </div>
            <div ref={list} id="palette-list" role="listbox" className="no-scrollbar fade-y max-h-[56vh] overflow-y-auto p-2">
              {results.length === 0 && (
                <p className="px-4 py-8 text-center text-[13.5px] text-ink/50">
                  {online.loading && q.trim().length >= 2
                    ? 'Searching online…'
                    : online.error && online.q === q.trim()
                      ? `Nothing in your library matches “${q}”. Online: ${online.error}`
                      : `Nothing matches “${q}”.`}
                </p>
              )}
              {online.loading && q.trim().length >= 2 && results.every((r) => r.c.group !== 'Online') && (
                <div className="px-3 pb-1 pt-3 text-[11.5px] font-medium text-ink/45">Online</div>
              )}
              {online.loading && q.trim().length >= 2 && results.every((r) => r.c.group !== 'Online') && (
                <div className="grid gap-1.5 px-3 py-2" aria-busy="true">
                  {Array.from({ length: 3 }, (_, i) => (
                    <div key={i} className="flex items-center gap-3">
                      <span className="shimmer h-3 w-2/3 rounded-full" />
                    </div>
                  ))}
                </div>
              )}
              {results.map(({ c, head }, i) => {
                const onlineHit = c.group === 'Online'
                return (
                  <div key={c.id}>
                    {head && <div className="px-3 pb-1 pt-3 text-[11.5px] font-medium text-ink/45">{head}</div>}
                    <button
                      data-i={i}
                      role="option"
                      aria-selected={i === sel}
                      onMouseMove={() => setSel(i)}
                      onClick={() => run(c)}
                      onContextMenu={(e) => (c.menu ? menuFromEvent(e, menuOf(c)) : e.preventDefault())}
                      className={`flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left ${
                        i === sel ? 'palette-on bg-[color-mix(in_oklab,var(--accent)_22%,transparent)]' : ''
                      }`}
                    >
                      <span className="min-w-0 flex-1">
                        <span className="flex min-w-0 items-center gap-1.5">
                          <span className="truncate text-[14px]">{c.label}</span>
                          {onlineHit && <AvailabilityBadge availability="online" />}
                        </span>
                        {c.detail && <span className="block truncate text-[12px] text-ink/50">{c.detail}</span>}
                      </span>
                      {c.keys && <kbd>{c.keys}</kbd>}
                    </button>
                  </div>
                )
              })}
              {results.length > 0 && !online.loading && online.error && online.q === q.trim() && (
                <p role="status" className="px-3 pb-1 pt-3 text-[12px] text-ink/45">
                  Online: {online.error}
                </p>
              )}
            </div>
            <div className="flex gap-4 border-t border-edge px-5 py-2.5 text-[11.5px] text-ink/45">
              <span>
                <kbd>↑</kbd> <kbd>↓</kbd> to move
              </span>
              <span>
                <kbd>Enter</kbd> to run
              </span>
              {results[sel]?.c.menu && (
                <span>
                  <kbd>→</kbd> more
                </span>
              )}
              <span className="ml-auto">
                <kbd>?</kbd> all shortcuts
              </span>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}

/** Every shortcut on one sheet, opened with "?". */
export function ShortcutSheet(props: { open: boolean; onClose: () => void }) {
  const rows: [string, string][] = [
    ['Ctrl+K', 'Command palette: go anywhere, find anything, run any action'],
    ['Ctrl+1 … Ctrl+5', 'Music, Health, Organize, Playlists, Devices'],
    ['Ctrl+,', 'Settings'],
    ['Ctrl+H', 'Activity (history and undo)'],
    ['/', 'Search your music'],
    ['Ctrl+Shift+L', 'Switch between light and dark'],
    ['1 2 3 · 0', 'On a question: answer · skip'],
    ['Enter', 'On a question: accept the recommendation'],
    ['Space', 'On a question: play the clips'],
    ['Ctrl+Enter', 'In Organize: accept every recommendation'],
    ['Space', 'Play or pause'],
    ['L', 'Lyrics'],
    ['F11', 'Lyrics on the big screen'],
    ['Ctrl+Shift+D', 'Desktop lyrics, from anywhere'],
    ['Ctrl+M', 'Mini player'],
    ['Esc', 'Close the sheet, palette or dialog'],
    ['?', 'This list'],
  ]
  return (
    <AnimatePresence>
      {props.open && (
        <motion.div
          className="fixed inset-0 z-50 grid place-items-center bg-black/30"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          onMouseDown={props.onClose}
        >
          <motion.div
            role="dialog"
            aria-label="Keyboard shortcuts"
            className="popover w-[min(560px,92vw)] rounded-[20px] p-6"
            initial={{ scale: 0.97, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            exit={{ scale: 0.97, opacity: 0 }}
            onMouseDown={(e) => e.stopPropagation()}
          >
            <h2 className="text-[20px] font-semibold tracking-[-0.02em]">Keyboard shortcuts</h2>
            <div className="mt-4 grid grid-cols-[auto_1fr] gap-x-5 gap-y-2.5 text-[13.5px]">
              {rows.map(([k, d]) => (
                <div key={k} className="contents">
                  <span className="text-right">
                    <kbd>{k}</kbd>
                  </span>
                  <span className="text-ink/80">{d}</span>
                </div>
              ))}
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}
