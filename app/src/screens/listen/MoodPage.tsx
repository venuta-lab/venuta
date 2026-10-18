import { motion } from 'motion/react'
import { useEffect, useState } from 'react'
import { Detail } from '../../components/ui'
import { onlineMood, type MoodTile, type OnlineCollection } from '../../lib/engine'
import { onlineProblem } from '../../lib/online'
import { spring } from './model'
import { CollectionCard } from './OnlineCards'

/*
  A mood or genre from the Search page: its name, then its playlists as a grid of cards. The
  catalog's color for it lights the top softly, like an album's cover does on its page.
*/

export function MoodPage({ tile, onBack }: { tile: MoodTile; onBack: () => void }) {
  const [lists, setLists] = useState<OnlineCollection[] | null>(null)
  const [problem, setProblem] = useState('')
  useEffect(() => {
    let live = true
    onlineMood(tile.params)
      .then((l) => live && setLists(l))
      .catch((e) => {
        if (!live) return
        setProblem(onlineProblem(e))
        setLists([])
      })
    return () => {
      live = false
    }
  }, [tile.params])
  const color = /^#[0-9a-f]{6}$/i.test(tile.color) ? tile.color : 'var(--accent)'

  return (
    <Detail title={tile.title} onBack={onBack}>
      <div
        aria-hidden
        className="pointer-events-none absolute -left-10 -right-10 -top-16 h-[320px] opacity-50"
        style={{ background: `radial-gradient(70% 100% at 20% 0%, color-mix(in oklab, ${color} 55%, transparent), transparent 70%)` }}
      />
      <motion.div className="relative" initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={spring}>
        <h1 className="title hero-title text-[40px] leading-[1.05]">{tile.title}</h1>
        <p className="mt-2 text-[13.5px] text-ink/60">
          {lists === null ? 'Reading its playlists…' : lists.length ? `${lists.length.toLocaleString('en-IN')} playlists from the online catalog` : ''}
        </p>
      </motion.div>
      <div className="relative mt-8">
        {problem ? (
          <p role="status" className="py-6 text-[13px] text-ink/55">
            {problem}
          </p>
        ) : lists === null ? (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(168px,1fr))] gap-x-5 gap-y-7" aria-hidden>
            {Array.from({ length: 10 }, (_, i) => (
              <div key={i}>
                <div className="shimmer aspect-square rounded-2xl" style={{ animationDelay: `${i * 80}ms` }} />
                <div className="shimmer mt-3 h-3 w-3/4 rounded-full" />
              </div>
            ))}
          </div>
        ) : !lists.length ? (
          <p className="py-6 text-[13px] text-ink/55">The online catalog has no playlists here right now.</p>
        ) : (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(168px,1fr))] gap-x-5 gap-y-7">
            {lists.map((c) => (
              <CollectionCard key={c.collection_id} collection={c} width={168} />
            ))}
          </div>
        )}
      </div>
    </Detail>
  )
}
