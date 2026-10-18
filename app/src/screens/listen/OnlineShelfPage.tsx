import { motion } from 'motion/react'
import { useEffect, useState } from 'react'
import { Detail } from '../../components/ui'
import { onlineShelfMore, type OnlineMore, type OnlineSuggestion } from '../../lib/engine'
import { onlineProblem } from '../../lib/online'
import { spring } from './model'
import { useOnlineCards } from './useOnlineCards'

/*
  Everything in a row of an online page, from its "See all": all of an artist's albums, all their
  singles, all their videos. The same cards as the row, as a grid; whose they are in the title.
*/

export function OnlineShelfPage({ title, owner, more, onBack }: { title: string; owner: string; more: OnlineMore; onBack: () => void }) {
  const [items, setItems] = useState<OnlineSuggestion[] | null>(null)
  const [problem, setProblem] = useState('')
  useEffect(() => {
    let live = true
    onlineShelfMore(more)
      .then((list) => live && setItems(list))
      .catch((e) => {
        if (!live) return
        setProblem(onlineProblem(e))
        setItems([])
      })
    return () => {
      live = false
    }
  }, [more])
  const cards = useOnlineCards(items ?? [], owner || title)
  const heading = owner ? `${owner}: ${title}` : title

  return (
    <Detail title={heading} onBack={onBack}>
      <motion.div initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={spring}>
        {owner && <p className="text-[13px] font-medium text-ink/60">{owner}</p>}
        <h1 className="title hero-title mt-1 text-[34px] leading-[1.1]">{title}</h1>
        {items && items.length > 0 && <p className="mt-2 text-[13px] text-ink/55">{items.length.toLocaleString('en-IN')}</p>}
      </motion.div>
      {problem ? (
        <p role="status" className="mt-8 py-6 text-[13px] text-ink/55">
          {problem}
        </p>
      ) : items === null ? (
        <p className="mt-8 py-6 text-[13px] text-ink/45">Reading the whole row from the online catalog…</p>
      ) : !items.length ? (
        <p className="mt-8 py-6 text-[13px] text-ink/55">The online catalog lists nothing more here.</p>
      ) : (
        <div className="mt-8 grid grid-cols-[repeat(auto-fill,168px)] gap-x-5 gap-y-7">
          {cards.map((c) => (
            <div key={c.key}>{c.node}</div>
          ))}
        </div>
      )}
    </Detail>
  )
}
