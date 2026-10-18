import { useEffect, useState, useSyncExternalStore } from 'react'
import { IconClose, IconSearch } from '../../components/Icons'
import { onlineMoods, type MoodSection } from '../../lib/engine'
import { recentSearches } from '../../lib/recentSearches'
import { MoodCard } from './OnlineCards'
import { useNav } from './model'
import { useLocalOnly } from '../../lib/localOnly'

/*
  The Search page before anything is searched: your recent searches (kept on this computer,
  each removable), then the catalog's moods and moments and its genres to browse. Offline, the
  browse part simply isn't there.
*/

const RECENT_SHOWN = 12

export function SearchHome({ onSearch, yours }: { onSearch: (words: string) => void; /** Your music's tiles, before the catalog's. */ yours?: React.ReactNode }) {
  const { openMood } = useNav()
  const recent = useSyncExternalStore(recentSearches.subscribe, recentSearches.list, recentSearches.list)
  const [sections, setSections] = useState<MoodSection[] | null>(null)
  const localOnly = useLocalOnly()
  useEffect(() => {
    if (localOnly) return
    let live = true
    onlineMoods()
      .then((s) => live && setSections(s))
      .catch(() => live && setSections([]))
    return () => {
      live = false
    }
  }, [localOnly])
  // Local only: no catalog moods and genres.
  const shown = localOnly ? [] : sections

  return (
    <div className="pb-16">
      {recent.length > 0 && (
        <section>
          <div className="mb-3 flex items-baseline gap-3">
            <h2 className="title text-[19px]">Recent searches</h2>
            <button onClick={() => recentSearches.clear()} className="ml-auto rounded-full px-3 py-1 text-[12.5px] text-ink/65 hover:bg-ink/[0.08] hover:text-ink">
              Clear
            </button>
          </div>
          <div className="flex flex-wrap gap-2">
            {recent.slice(0, RECENT_SHOWN).map((r) => (
              <span key={r.q} className="group flex items-center rounded-full bg-ink/[0.06] shadow-[inset_0_0_0_1px_var(--edge)] transition-colors hover:bg-ink/[0.1]">
                <button onClick={() => onSearch(r.q)} className="flex items-center gap-1.5 py-1.5 pl-3 pr-1 text-[13px]">
                  <IconSearch width={13} height={13} className="text-ink/45" />
                  {r.q}
                </button>
                <button
                  onClick={() => recentSearches.remove(r.q)}
                  aria-label={`Remove “${r.q}” from recent searches`}
                  title="Remove"
                  className="mr-1 grid h-6 w-6 place-items-center rounded-full text-ink/40 hover:bg-ink/10 hover:text-ink"
                >
                  <IconClose width={11} height={11} />
                </button>
              </span>
            ))}
          </div>
        </section>
      )}
      {yours}
      {shown === null ? (
        <div className={recent.length ? 'mt-10' : ''} aria-hidden>
          <div className="shimmer mb-4 h-4 w-40 rounded-full" />
          <div className="grid grid-cols-[repeat(auto-fill,minmax(176px,1fr))] gap-3">
            {Array.from({ length: 8 }, (_, i) => (
              <div key={i} className="shimmer aspect-[16/9] rounded-2xl" style={{ animationDelay: `${i * 60}ms` }} />
            ))}
          </div>
        </div>
      ) : (
        shown.map((s, i) => (
          <section key={s.title} className={i > 0 || recent.length ? 'mt-10' : ''}>
            <h2 className="title mb-3.5 text-[19px]">{s.title}</h2>
            <div className="grid grid-cols-[repeat(auto-fill,minmax(176px,1fr))] gap-3">
              {s.tiles.map((t) => (
                <MoodCard key={t.params} tile={t} onOpen={() => openMood(t)} />
              ))}
            </div>
          </section>
        ))
      )}
      {shown !== null && !shown.length && !recent.length && (
        <p className="py-16 text-center text-[14px] text-ink/55">Search your music and the online catalog. Press / to start typing.</p>
      )}
    </div>
  )
}
