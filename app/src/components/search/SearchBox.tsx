import { useEffect, useId, useState, useSyncExternalStore } from 'react'
import { recentSearches } from '../../lib/recentSearches'
import { IconClose, IconSearch } from '../Icons'

/*
  A search field with a panel under it: your recent searches when it's empty (kept on this
  computer only, each one removable), and quick suggestions from your library while you type.
  Picking either fills the field and runs the search. Enter runs (and remembers) what's typed.
*/

/** A quick suggestion: what it is, what it says, and the words it puts in the field. */
export interface Suggestion {
  key: string
  kind: 'Song' | 'Artist' | 'Album'
  label: string
  detail?: string
  /** The search it runs. */
  value: string
}

const SUGGEST_AFTER_MS = 180
const RECENT_SHOWN = 8

export function SearchBox(props: {
  id: string
  value: string
  onChange: (value: string) => void
  /** The words are meant (Enter, a recent or a suggestion picked): run them. Remembered here. */
  onSubmit?: (value: string) => void
  /** Escape with the panel closed: clear and leave the field. */
  onEscape?: () => void
  /** Library suggestions for the words (instant, local); none when left out. */
  suggest?: (text: string) => Suggestion[]
  placeholder: string
  label: string
  inputRef?: React.Ref<HTMLInputElement>
  /** A hint at the end of the field ("/"). */
  hint?: React.ReactNode
  className?: string
}) {
  const { inputRef } = props
  const listId = useId()
  const recent = useSyncExternalStore(recentSearches.subscribe, recentSearches.list, recentSearches.list)
  const [focused, setFocused] = useState(false)
  // Closed by Escape or a pick, until the words change or the field is used again.
  const [closedFor, setClosedFor] = useState<string | null>(null)
  const [active, setActive] = useState(-1)
  const text = props.value.trim()

  // Suggestions for the words, a moment after typing stops.
  const [suggested, setSuggested] = useState<{ for: string; items: Suggestion[] }>({ for: '', items: [] })
  const { suggest } = props
  useEffect(() => {
    if (!text || !suggest) return
    const t = window.setTimeout(() => setSuggested({ for: text, items: suggest(text) }), SUGGEST_AFTER_MS)
    return () => window.clearTimeout(t)
  }, [text, suggest])

  const rows: { key: string; kind: 'recent' | Suggestion['kind']; label: string; detail?: string; value: string }[] = text
    ? suggested.for === text
      ? suggested.items
      : []
    : recent.slice(0, RECENT_SHOWN).map((r) => ({ key: `recent:${r.q}`, kind: 'recent' as const, label: r.q, value: r.q }))
  const open = focused && closedFor !== props.value && rows.length > 0
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
              run(at >= 0 ? rows[at].value : props.value)
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
          className="min-w-0 flex-1 bg-transparent py-2 text-[13.5px] text-ink outline-none placeholder:text-ink/45 [&::-webkit-search-cancel-button]:hidden"
        />
        {props.hint}
      </div>
      {open && (
        <div
          // Keeps the field focused while the panel is used.
          onMouseDown={(e) => e.preventDefault()}
          className="popover absolute left-0 right-0 top-[calc(100%+6px)] z-40 overflow-hidden rounded-2xl p-1.5"
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
                onClick={() => run(r.value)}
                className={`group flex cursor-pointer items-center gap-2.5 rounded-xl px-3 py-2 text-[13.5px] ${i === at ? 'bg-ink/[0.08]' : ''}`}
              >
                {r.kind !== 'recent' && <span className="w-12 shrink-0 text-[11.5px] text-ink/45">{r.kind}</span>}
                <span className="min-w-0 flex-1 truncate">
                  {r.label}
                  {r.detail && <span className="text-ink/50"> · {r.detail}</span>}
                </span>
                {r.kind === 'recent' && (
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
