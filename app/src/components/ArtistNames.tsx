import { useEffect, useState } from 'react'
import { getAliases, saveAliases, type Aliases } from '../lib/engine'

/**
 * Your artist names: one spelling per person, the spellings that mean them, and names kept
 * apart. Saving changes the rule only; Organize then shows what it changes in your library.
 */
export function ArtistNames(props: { onSaved?: (previous: Aliases) => void }) {
  const [view, setView] = useState<Aliases | null>(null)
  const [q, setQ] = useState('')
  const [status, setStatus] = useState('')
  const [editing, setEditing] = useState<{ i: number; name: string } | null>(null)
  useEffect(() => {
    getAliases()
      .then(setView)
      .catch((e) => setStatus(String(e)))
  }, [])
  async function save(next: Aliases, message: string) {
    const previous = view
    setView(next)
    await saveAliases(next)
    setStatus(message)
    window.setTimeout(() => setStatus(''), 2600)
    if (previous) props.onSaved?.(previous)
  }
  function rename(i: number, to: string) {
    if (!view) return
    const from = view.artists[i].name
    const name = to.trim()
    setEditing(null)
    if (!name || name === from) return
    if (view.artists.some((a, j) => j !== i && a.name.toLowerCase() === name.toLowerCase())) {
      setStatus(`${name} is already a name in your list.`)
      return
    }
    // The old spelling stays recognised as the same person.
    const artists = view.artists.map((a, j) =>
      j === i ? { name, variants: [...new Set([from, ...a.variants.filter((v) => v !== name)])] } : a,
    )
    void save({ ...view, artists }, `Now written as ${name}.`)
  }
  if (!view) return <p className="text-[13px] text-ink/55">{status || 'Reading your artist names…'}</p>
  const ql = q.trim().toLowerCase()
  const shown = view.artists
    .map((a, i) => ({ a, i }))
    .filter(({ a }) => !ql || [a.name, ...a.variants].some((n) => n.toLowerCase().includes(ql)))
    .sort((x, y) => x.a.name.localeCompare(y.a.name))
  return (
    <div>
      <div className="flex items-center gap-3">
        <input
          id="aliases-search"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          placeholder={`Search ${view.artists.length} names`}
          className="min-w-0 flex-1 rounded-xl bg-ink/[0.06] px-3.5 py-2 text-[13px] outline-none focus:ring-1 focus:ring-[var(--accent)]"
        />
        {status && <span className="text-[12.5px] text-good">{status}</span>}
      </div>
      <div className="mt-3 grid max-h-[420px] gap-1 overflow-y-auto pr-1">
        {shown.map(({ a, i }) => (
          <div key={a.name} className="group flex flex-wrap items-center gap-2 rounded-xl px-3 py-2 hover:bg-ink/[0.04]">
            {editing?.i === i ? (
              <input
                autoFocus
                value={editing.name}
                onChange={(e) => setEditing({ i, name: e.target.value })}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') rename(i, editing.name)
                  if (e.key === 'Escape') setEditing(null)
                }}
                onBlur={() => rename(i, editing.name)}
                aria-label={`New spelling for ${a.name}`}
                className="min-w-[180px] rounded-lg bg-ink/[0.08] px-2 py-0.5 text-[13.5px] font-medium outline-none ring-1 ring-[var(--accent)]"
              />
            ) : (
              <button
                onClick={() => setEditing({ i, name: a.name })}
                title="Change how this name is written"
                className="min-w-[180px] text-left text-[13.5px] font-medium hover:text-accent-ink"
              >
                {a.name}
              </button>
            )}
            <span className="flex min-w-0 flex-1 flex-wrap gap-1.5">
              {a.variants.map((v) => (
                <span key={v} className="inline-flex items-center gap-1 rounded-full bg-ink/[0.07] py-0.5 pl-2.5 pr-1 text-[12px] text-ink/75">
                  {v}
                  <button
                    aria-label={`Stop treating ${v} as ${a.name}`}
                    title="Stop treating this spelling as the same person"
                    onClick={() => {
                      const artists = view.artists.map((x, j) => (j === i ? { ...x, variants: x.variants.filter((y) => y !== v) } : x))
                      void save({ ...view, artists }, `“${v}” is no longer treated as ${a.name}.`)
                    }}
                    className="grid h-4 w-4 place-items-center rounded-full text-ink/50 hover:bg-ink/15 hover:text-ink"
                  >
                    ×
                  </button>
                </span>
              ))}
            </span>
          </div>
        ))}
        {shown.length === 0 && <p className="py-6 text-center text-[13px] text-ink/50">No names match “{q}”.</p>}
      </div>
      {view.distinct.length > 0 && (
        <>
          <h4 className="mt-6 text-[14px] font-medium">Kept apart</h4>
          <p className="mt-1 text-[12.5px] text-ink/55">Names you said are different people. They are never suggested as one again.</p>
          <div className="mt-2 grid gap-1">
            {view.distinct.map(([x, y], i) => (
              <div key={`${x}|${y}`} className="flex items-center gap-3 rounded-xl px-3 py-2 text-[13.5px] hover:bg-ink/[0.04]">
                <span className="flex-1">
                  {x} <span className="text-ink/45">and</span> {y}
                </span>
                <button
                  onClick={() => void save({ ...view, distinct: view.distinct.filter((_, j) => j !== i) }, 'They can be suggested as one artist again.')}
                  className="rounded-full px-3 py-1 text-[12.5px] text-ink/60 hover:bg-ink/10"
                >
                  Ask again
                </button>
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  )
}
