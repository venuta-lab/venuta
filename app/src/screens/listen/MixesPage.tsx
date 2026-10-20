import { motion } from 'motion/react'
import { useEffect, useState } from 'react'
import { Detail } from '../../components/ui'
import { onlineMixedForYou, type OnlineShelf } from '../../lib/engine'
import { onlineProblem } from '../../lib/online'
import { useYtmStatus } from '../../lib/ytmLibrary'
import { spring } from './model'
import { useOnlineCards } from './useOnlineCards'

/*
  Mixed for you, all of it: from Home's shelf, every mix YouTube Music made for the account
  (My Supermix, the My Mixes, Discover, Replay and the rest, as it lists them), in its rows and
  order. Each card plays the way its own play button plays it (lib/mixes.ts). Asked for the way
  Home is, so it follows "Recommendations for me"; signed out or in Local only nothing is asked.
*/

/** One row of the page: its heading, when YouTube gave it one, and its mixes as a grid. */
function MixRow({ shelf, label }: { shelf: OnlineShelf; label: string }) {
  const cards = useOnlineCards(shelf.items, label)
  return (
    <section className="mt-8">
      {shelf.title && <h2 className="title mb-3.5 text-[19px]">{shelf.title}</h2>}
      <div className="grid grid-cols-[repeat(auto-fill,168px)] gap-x-5 gap-y-7">
        {cards.map((c) => (
          <div key={c.key}>{c.node}</div>
        ))}
      </div>
    </section>
  )
}

export function MixesPage({ title, onBack }: { title: string; onBack: () => void }) {
  const { signed_in } = useYtmStatus()
  const [rows, setRows] = useState<OnlineShelf[] | null>(null)
  const [problem, setProblem] = useState('')
  useEffect(() => {
    if (!signed_in) return
    let live = true
    onlineMixedForYou()
      .then((found) => live && setRows(found))
      .catch((e) => {
        if (!live) return
        setProblem(onlineProblem(e))
        setRows([])
      })
    return () => {
      live = false
    }
  }, [signed_in])
  const count = rows?.reduce((n, r) => n + r.items.length, 0) ?? 0

  return (
    <Detail title={title} onBack={onBack}>
      <motion.div initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={spring}>
        <p className="text-[13px] font-medium text-ink/60">YouTube Music</p>
        <h1 className="title hero-title mt-1 text-[34px] leading-[1.1]">{title}</h1>
        {count > 0 && <p className="mt-2 text-[13px] text-ink/55">{count === 1 ? '1 mix' : `${count.toLocaleString('en-IN')} mixes`}</p>}
      </motion.div>
      {!signed_in ? (
        <p className="mt-8 py-6 text-[13px] text-ink/55">Sign in to YouTube Music (Settings, Online) to see the mixes it makes for you.</p>
      ) : problem ? (
        <p role="status" className="mt-8 py-6 text-[13px] text-ink/55">
          {problem}
        </p>
      ) : rows === null ? (
        <p className="mt-8 py-6 text-[13px] text-ink/45">Reading your mixes from YouTube Music…</p>
      ) : !rows.length ? (
        <p className="mt-8 py-6 text-[13px] text-ink/55">YouTube Music has no mixes for you right now.</p>
      ) : (
        rows.map((r, i) => <MixRow key={`${r.title}:${i}`} shelf={r} label={title} />)
      )}
    </Detail>
  )
}
