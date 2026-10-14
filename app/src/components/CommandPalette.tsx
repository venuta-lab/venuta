import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { IconSearch } from './Icons'
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

export function CommandPalette(props: { open: boolean; onClose: () => void; commands: Command[] }) {
  const [q, setQ] = useState('')
  const [sel, setSel] = useState(0)
  const input = useRef<HTMLInputElement>(null)
  const list = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (props.open) {
      setQ('')
      setSel(0)
      window.setTimeout(() => input.current?.focus(), 0)
    }
  }, [props.open])

  const results = useMemo(() => {
    const words = searchKey(q).split(' ').filter(Boolean)
    const has = matcher(q)
    const ranked = props.commands
      .map((c) => ({ c, s: score(c, words, has) }))
      .filter((x) => x.s > 0)
    // Without a query, show actions first and keep albums out of the way.
    const shown = q.trim()
      ? ranked.sort((a, b) => b.s - a.s).slice(0, 40)
      : ranked.filter((x) => x.c.group !== 'Albums')
    // Show a group heading above the first command of each group.
    return shown.map((x, i) => ({ c: x.c, head: i === 0 || shown[i - 1].c.group !== x.c.group ? x.c.group : '' }))
  }, [props.commands, q])

  useEffect(() => {
    list.current?.querySelector(`[data-i="${sel}"]`)?.scrollIntoView({ block: 'nearest' })
  }, [sel])

  function run(c: Command | undefined) {
    if (!c) return
    props.onClose()
    window.setTimeout(c.run, 0)
  }

  function onKey(e: React.KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setSel((s) => Math.min(s + 1, results.length - 1))
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setSel((s) => Math.max(s - 1, 0))
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
                placeholder="Type a command, an album or an artist"
                className="min-w-0 flex-1 bg-transparent text-[16px] text-ink outline-none placeholder:text-ink/40"
                role="combobox"
                aria-expanded="true"
                aria-controls="palette-list"
              />
              <kbd>Esc</kbd>
            </div>
            <div ref={list} id="palette-list" role="listbox" className="no-scrollbar fade-y max-h-[56vh] overflow-y-auto p-2">
              {results.length === 0 && (
                <p className="px-4 py-8 text-center text-[13.5px] text-ink/50">Nothing matches “{q}”.</p>
              )}
              {results.map(({ c, head }, i) => {
                return (
                  <div key={c.id}>
                    {head && <div className="px-3 pb-1 pt-3 text-[11.5px] font-medium text-ink/45">{head}</div>}
                    <button
                      data-i={i}
                      role="option"
                      aria-selected={i === sel}
                      onMouseMove={() => setSel(i)}
                      onClick={() => run(c)}
                      className={`flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left ${
                        i === sel ? 'palette-on bg-[color-mix(in_oklab,var(--accent)_22%,transparent)]' : ''
                      }`}
                    >
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-[14px]">{c.label}</span>
                        {c.detail && <span className="block truncate text-[12px] text-ink/50">{c.detail}</span>}
                      </span>
                      {c.keys && <kbd>{c.keys}</kbd>}
                    </button>
                  </div>
                )
              })}
            </div>
            <div className="flex gap-4 border-t border-edge px-5 py-2.5 text-[11.5px] text-ink/45">
              <span>
                <kbd>↑</kbd> <kbd>↓</kbd> to move
              </span>
              <span>
                <kbd>Enter</kbd> to run
              </span>
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
