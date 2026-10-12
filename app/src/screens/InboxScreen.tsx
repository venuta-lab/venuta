import { motion } from 'motion/react'
import { useEffect, useState } from 'react'
import { ReviewScreen } from './ReviewScreen'
import { IncomingScreen } from './IncomingScreen'

const spring = { type: 'spring', stiffness: 380, damping: 34 } as const

/** Everything waiting for you: questions and what was handled, and new music. */
export function InboxScreen(props: { root: string; onImported: () => void; onCount: (n: number) => void }) {
  const [tab, setTab] = useState<'decide' | 'new'>('decide')
  useEffect(() => props.onCount(0), [props])
  return (
    <div className="mx-auto max-w-[1080px]">
      <header className="flex flex-wrap items-end gap-6 pb-4 pt-6">
        <div className="min-w-0 flex-1">
          <h1 className="text-[40px] font-semibold leading-none tracking-[-0.035em]">Inbox</h1>
          <p className="mt-2.5 text-[14px] text-ink/60">Everything waiting for you, in one place.</p>
        </div>
      </header>
      <div role="tablist" className="mb-2 inline-flex rounded-full bg-ink/[0.07] p-1">
        {(
          [
            ['decide', 'To decide'],
            ['new', 'New music'],
          ] as const
        ).map(([id, label]) => (
          <button
            key={id}
            role="tab"
            aria-selected={tab === id}
            onClick={() => setTab(id)}
            className={`relative rounded-full px-4 py-1.5 text-[13px] ${tab === id ? 'text-ink' : 'text-ink/60 hover:text-ink'}`}
          >
            {tab === id && <motion.span layoutId="inbox-tab" transition={spring} className="absolute inset-0 rounded-full bg-ink/[0.14]" />}
            <span className="relative">{label}</span>
          </button>
        ))}
      </div>
      {tab === 'decide' ? <ReviewScreen root={props.root} /> : <IncomingScreen root={props.root} onImported={props.onImported} />}
    </div>
  )
}
