import { useEffect, useId, useState, useSyncExternalStore, type ReactNode } from 'react'
import { recentSearches } from '../../lib/recentSearches'
import { AvailabilityBadge } from '../AvailabilityBadge'
import { IconClose, IconSearch } from '../Icons'

/*
  A search field with a panel under it. Empty: your recent searches (kept on this computer
  only, each one removable). While you type: things that match, your own first (an artist, an
  album, a song, each with its picture and what it is), then the online catalog's, then
  completions of the words, then "All results for …". Picking a thing opens or plays it;
  picking words (or Enter) searches for them. Up and Down move through it all. A completion that
  is one of your own past YouTube Music searches (Signed-in search) says so, and can be taken out
  of that history with its × (asked first).
*/

/** A thing that matches: what it is, what it says, its picture, and what picking it does. */
export interface Suggestion {
  key: string
  kind: 'Song' | 'Artist' | 'Album' | 'Playlist'
  label: string
  detail?: string
  /** The search it runs when it has no `onPick`. */
  value: string
  /** Its picture (a cover, an avatar). */
  art?: ReactNode
  /** Round picture (an artist). */
  round?: boolean
  /** In your library (a quiet badge says so) or from the online catalog. */
  where?: 'library' | 'online'
  /** Open or play it instead of searching. */
  onPick?: () => void
}

/** What the online catalog suggests for the words: things, and completions of the words. */
export interface OnlineSuggestions {
  items: Suggestion[]
  queries: string[]
  /** Completions that are your own past searches there, and how to take each out (true: it went). */
  past?: PastQuery[]
}

/** One of your past searches on the account, as a completion. */
export interface PastQuery {
  text: string
  remove?: () => Promise<boolean>
}

const SUGGEST_AFTER_MS = 120
const ONLINE_AFTER_MS = 150
const RECENT_SHOWN = 8
/** Things shown at most (yours first, then the catalog's), and completions. */
const THINGS_SHOWN = 6
const QUERIES_SHOWN = 6

type Row =
  | { key: string; type: 'recent'; label: string; value: string }
  | { key: string; type: 'thing'; s: Suggestion }
  | { key: string; type: 'query'; label: string; value: string; past?: PastQuery }
  | { key: string; type: 'all'; label: string; value: string }

export function SearchBox(props: {
  id: string
  value: string
  onChange: (value: string) => void
  /** The words are meant (Enter, a recent or completion picked): run them. Remembered here. */
  onSubmit?: (value: string) => void
  /** Escape with the panel closed: clear and leave the field. */
  onEscape?: () => void
  /** Library suggestions for the words (instant, local); none when left out. */
  suggest?: (text: string) => Suggestion[]
  /** The online catalog's suggestions for the words; none when left out. */
  onlineSuggest?: (text: string) => Promise<OnlineSuggestions>
  placeholder: string
  label: string
  inputRef?: React.Ref<HTMLInputElement>
  /** A hint at the end of the field ("/"). */
  hint?: React.ReactNode
  className?: string
  /** Slimmer, for the title bar. */
  compact?: boolean
}) {
  const { inputRef } = props
  const listId = useId()
  const recent = useSyncExternalStore(recentSearches.subscribe, recentSearches.list, recentSearches.list)
  const [focused, setFocused] = useState(false)
  // Closed by Escape or a pick, until the words change or the field is used again.
  const [closedFor, setClosedFor] = useState<string | null>(null)
  const [active, setActive] = useState(-1)
  const text = props.value.trim()

  // Your library's matches, a moment after typing stops.
  const [suggested, setSuggested] = useState<{ for: string; items: Suggestion[] }>({ for: '', items: [] })
  const { suggest, onlineSuggest } = props
  useEffect(() => {
    if (!text || !suggest) return
    const t = window.setTimeout(() => setSuggested({ for: text, items: suggest(text) }), SUGGEST_AFTER_MS)
    return () => window.clearTimeout(t)
  }, [text, suggest])

  // The online catalog's, a moment later; an answer for older words is dropped.
  const [online, setOnline] = useState<{ for: string; found: OnlineSuggestions }>({ for: '', found: { items: [], queries: [] } })
  // Past searches taken out of the account's history this time: gone from the panel at once.
  const [gonePast, setGonePast] = useState<ReadonlySet<string>>(new Set())
  useEffect(() => {
    if (!text || !onlineSuggest) return
    let live = true
    const t = window.setTimeout(() => {
      onlineSuggest(text)
        .then((found) => live && setOnline({ for: text, found }))
        .catch(() => {})
    }, ONLINE_AFTER_MS)
    return () => {
      live = false
      window.clearTimeout(t)
    }
  }, [text, onlineSuggest])

  const rows: Row[] = (() => {
    if (!text) return recent.slice(0, RECENT_SHOWN).map((r) => ({ key: `recent:${r.q}`, type: 'recent' as const, label: r.q, value: r.q }))
    const mine = suggested.for === text ? suggested.items : []
    const theirs = online.for === text ? online.found : { items: [], queries: [] }
    // Yours first; the catalog's fill the rest, without one you already have by name.
    const seen = new Set(mine.map((s) => `${s.kind}:${s.label.toLowerCase()}`))
    const things = [...mine, ...theirs.items.filter((s) => !seen.has(`${s.kind}:${s.label.toLowerCase()}`))].slice(0, THINGS_SHOWN)
    const queries = theirs.queries.filter((q) => q.trim().toLowerCase() !== text.toLowerCase() && !gonePast.has(q)).slice(0, QUERIES_SHOWN)
    const pastOf = (q: string) => theirs.past?.find((p) => p.text === q)
    return [
      ...things.map((s) => ({ key: `thing:${s.key}`, type: 'thing' as const, s })),
      ...queries.map((q) => ({ key: `query:${q}`, type: 'query' as const, label: q, value: q, past: pastOf(q) })),
      { key: 'all', type: 'all' as const, label: `All results for “${text}”`, value: props.value },
    ]
  })()
  const open = focused && closedFor !== props.value && (text ? rows.length > 1 || !!props.onSubmit : rows.length > 0)
  const at = open && active < rows.length ? active : -1

  // Scrolling the page closes the panel, so it never sits over the results being looked at.
  const value = props.value
  useEffect(() => {
    if (!open) return
    const close = () => setClosedFor(value)
    window.addEventListener('scroll', close, { capture: true, passive: true })
    return () => window.removeEventListener('scroll', close, { capture: true })
  }, [open, value])

  const run = (value: string) => {
    const v = value.trim()
    if (!v) return
    recentSearches.push(v)
    props.onChange(value)
    props.onSubmit?.(v)
    setClosedFor(value)
    setActive(-1)
  }
  const pick = (row: Row) => {
    if (row.type === 'thing' && row.s.onPick) {
      // Opening or playing something found by these words means them: they're remembered.
      if (text) recentSearches.push(text)
      row.s.onPick()
      setClosedFor(props.value)
      setActive(-1)
      return
    }
    run(row.type === 'thing' ? row.s.value : row.value)
  }

  const firstQuery = rows.findIndex((r) => r.type === 'query')
  const allAt = rows.findIndex((r) => r.type === 'all')

  return (
    <div className={`relative ${props.className ?? ''}`}>
      <div className="flex items-center gap-2 rounded-full bg-ink/[0.07] px-4 shadow-[inset_0_0_0_1px_var(--edge)] focus-within:bg-ink/[0.1] focus-within:shadow-[inset_0_0_0_1px_color-mix(in_oklab,var(--accent)_60%,transparent)]">
        <IconSearch className="shrink-0 text-ink/45" />
        <input
          ref={inputRef}
          id={props.id}
          type="search"
          role="combobox"
          aria-label={props.label}
          aria-expanded={open}
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={at >= 0 ? `${listId}-${at}` : undefined}
          value={props.value}
          autoComplete="off"
          spellCheck={false}
          onFocus={() => {
            setFocused(true)
            setClosedFor(null)
          }}
          onBlur={() => {
            setFocused(false)
            setActive(-1)
          }}
          onChange={(e) => {
            props.onChange(e.target.value)
            setClosedFor(null)
            setActive(-1)
          }}
          onKeyDown={(e) => {
            if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
              if (!rows.length) return
              e.preventDefault()
              if (!open) setClosedFor(null)
              const n = rows.length
              setActive((i) => (e.key === 'ArrowDown' ? (i + 1) % n : i <= 0 ? n - 1 : i - 1))
            } else if (e.key === 'Enter') {
              e.preventDefault()
              if (at >= 0) pick(rows[at])
              else run(props.value)
            } else if (e.key === 'Escape') {
              if (open) {
                e.preventDefault()
                e.stopPropagation()
                setClosedFor(props.value)
                setActive(-1)
              } else {
                props.onEscape?.()
                e.currentTarget.blur()
              }
            }
          }}
          placeholder={props.placeholder}
          className={`min-w-0 flex-1 bg-transparent text-ink outline-none placeholder:text-ink/45 [&::-webkit-search-cancel-button]:hidden ${props.compact ? 'py-[5px] text-[12.5px]' : 'py-2 text-[13.5px]'}`}
        />
        {props.hint}
      </div>
      {open && (
        <div
          // Keeps the field focused while the panel is used.
          onMouseDown={(e) => e.preventDefault()}
          className="popover absolute left-0 right-0 top-[calc(100%+6px)] z-40 overflow-y-auto overscroll-contain rounded-2xl p-1.5"
          // Never under the player bar: a long list scrolls inside the panel.
          style={{ minWidth: props.compact ? 420 : 340, maxHeight: 'min(560px, calc(100vh - 260px))' }}
        >
          {!text && <div className="px-3 pb-1 pt-1.5 text-[11.5px] font-medium text-ink/45">Recent searches</div>}
          <ul id={listId} role="listbox" aria-label={text ? 'Suggestions' : 'Recent searches'} className="grid gap-0.5">
            {rows.map((r, i) => (
              <li
                key={r.key}
                id={`${listId}-${i}`}
                role="option"
                aria-selected={i === at}
                onMouseMove={() => setActive(i)}
                onClick={() => pick(r)}
                className={`group flex cursor-pointer items-center gap-3 rounded-xl px-2.5 text-[13.5px] ${
                  r.type === 'thing' ? 'py-1.5' : 'py-2'
                } ${i === at ? 'bg-ink/[0.08]' : ''} ${(i === firstQuery && i > 0) || (i === allAt && i > 0 && firstQuery < 0) ? 'mt-1 border-t border-edge pt-2.5' : ''}`}
              >
                {r.type === 'thing' ? (
                  <>
                    <span
                      className={`grid h-10 w-10 shrink-0 place-items-center overflow-hidden bg-ink/[0.07] text-[15px] text-ink/40 ${r.s.round ? 'rounded-full' : 'rounded-lg'}`}
                    >
                      {r.s.art ?? r.s.label.slice(0, 1)}
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="block truncate font-medium">{r.s.label}</span>
                      <span className="block truncate text-[12px] text-ink/55">{[r.s.kind, r.s.detail].filter(Boolean).join(' · ')}</span>
                    </span>
                    {r.s.where === 'library' && <AvailabilityBadge availability="local" />}
                  </>
                ) : (
                  <>
                    <IconSearch width={15} height={15} className="shrink-0 text-ink/45" />
                    <span className={`min-w-0 flex-1 truncate ${r.type === 'all' ? 'text-[13px] text-ink/65' : ''}`}>{r.label}</span>
                    {r.type === 'query' && r.past && <span className="shrink-0 text-[11.5px] text-ink/45">Your past search</span>}
                    {r.type === 'query' && r.past?.remove && (
                      <button
                        tabIndex={-1}
                        aria-label={`Remove “${r.label}” from your YouTube Music search history`}
                        title="Remove from your YouTube Music search history"
                        onClick={(e) => {
                          e.stopPropagation()
                          const text = r.label
                          void r.past?.remove?.().then((gone) => gone && setGonePast((g) => new Set(g).add(text)))
                        }}
                        className="grid h-6 w-6 shrink-0 place-items-center rounded-full text-ink/40 opacity-0 hover:bg-ink/10 hover:text-ink group-hover:opacity-100"
                      >
                        <IconClose width={12} height={12} />
                      </button>
                    )}
                    {r.type === 'recent' && (
                      <button
                        tabIndex={-1}
                        aria-label={`Remove “${r.label}” from recent searches`}
                        title="Remove"
                        onClick={(e) => {
                          e.stopPropagation()
                          recentSearches.remove(r.value)
                        }}
                        className="grid h-6 w-6 shrink-0 place-items-center rounded-full text-ink/40 opacity-0 hover:bg-ink/10 hover:text-ink group-hover:opacity-100"
                      >
                        <IconClose width={12} height={12} />
                      </button>
                    )}
                  </>
                )}
              </li>
            ))}
          </ul>
          {!text && (
            <div className="flex items-center justify-between px-3 pb-1 pt-2 text-[11.5px] text-ink/40">
              <span>Kept on this computer only.</span>
              <button onClick={() => recentSearches.clear()} className="rounded-md px-1.5 py-0.5 font-medium text-ink/55 hover:bg-ink/10 hover:text-ink">
                Clear all
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  )
}
