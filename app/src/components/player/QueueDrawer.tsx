import { useEscape } from '../../lib/layers'
import { AnimatePresence, motion } from 'motion/react'
import { useMemo, useState } from 'react'
import { clock, enqueueItems, setShuffle, usePlayer } from '../../lib/player'
import { useDropTarget } from '../../lib/drag'
import { notify } from '../../lib/notify'
import { IconClose } from '../Icons'
import { Segmented } from '../ui'
import { QueueRadio } from './QueueRadio'
import { QueueChips, QueueFrom, QueueList, QueueNote, RelatedList, SaveQueueButton } from './UpNext'
import { roomQueueLine, roomWords, useTogether } from '../../lib/together'
import { RoomQueue } from '../together/RoomQueue'

const spring = { type: 'spring', stiffness: 380, damping: 36 } as const

/**
 * What's playing and what comes next (components/player/UpNext.tsx draws the songs): click to
 * play, drag (or Alt+↑/↓) to reorder, × or Delete to remove; Related beside it. In a room, the
 * room's queue, the same for everyone.
 */
export function QueueDrawer(props: { open: boolean; onClose: () => void }) {
  const s = usePlayer()
  // In a room, the drawer shows the room's queue (the same for everyone).
  const v = useTogether()
  const room = (v.state === 'hosting' || v.state === 'in' || v.state === 'reconnecting') && v.room ? v : null
  const [tab, setTab] = useState<'next' | 'related'>('next')
  const { open, onClose } = props
  useEscape(open, onClose)

  const upcoming = s.queue.length - s.index - 1
  // Music dropped on the open queue joins its end.
  const drop = useDropTarget(
    open
      ? {
          label: 'Add to the queue',
          drop: async (c) => {
            await enqueueItems(s.root ?? '', await c.songs(), false)
            notify(`Added ${c.label} to the queue.`)
          },
        }
      : null,
  )
  const left = useMemo(() => s.queue.slice(s.index + 1).reduce((t, q) => t + q.duration_ms, 0), [s.queue, s.index])

  return (
    <AnimatePresence>
      {props.open && (
        <motion.aside
          {...drop.props}
          aria-label="Queue"
          initial={{ x: 420, opacity: 0 }}
          animate={{ x: 0, opacity: 1 }}
          exit={{ x: 420, opacity: 0 }}
          transition={spring}
          className="popover absolute bottom-3 right-3 top-3 z-40 flex w-[380px] flex-col rounded-[22px]"
        >
          <header className="flex items-start gap-3 px-5 pb-2 pt-4">
            <div className="min-w-0 flex-1">
              <h2 className="title text-[18px]">Queue</h2>
              {room ? (
                <>
                  <p className="mt-0.5 truncate text-[12px] text-ink/55">{roomWords(room)}</p>
                  <p className="mt-0.5 truncate text-[12px] text-ink/55">{roomQueueLine(room)}</p>
                </>
              ) : (
                <>
                  <QueueFrom className="mt-0.5 block truncate text-[12px] text-ink/55" />
                  <p className="mt-0.5 truncate text-[12px] text-ink/55">
                    {upcoming > 0 ? `${upcoming.toLocaleString('en-IN')} up next, ${clock(left)}` : 'Nothing after this song'}
                  </p>
                  <QueueRadio className="mt-1.5" />
                </>
              )}
              {/* Shuffled, what comes next isn't the order of the list it came from: say so, with the way back. */}
              {!room && s.shuffle && upcoming > 0 && (
                <p className="mt-1 flex items-center gap-2 text-[12px] text-ink/55">
                  <span>Shuffled</span>
                  <button onClick={() => void setShuffle(false)} className="rounded-full px-2 py-0.5 text-accent-ink hover:bg-ink/[0.08]">
                    Play in order
                  </button>
                </p>
              )}
            </div>
            {!room && <SaveQueueButton className="mt-0.5" />}
            <button aria-label="Close the queue" onClick={props.onClose} className="grid h-8 w-8 place-items-center rounded-full text-ink/60 hover:bg-ink/10 hover:text-ink">
              <IconClose />
            </button>
          </header>
          {!room && s.queue.length > 0 && (
            <div className="px-5 pb-2">
              <Segmented
                tabs
                size="sm"
                label="Queue"
                value={tab}
                onChange={setTab}
                options={[
                  { value: 'next', label: 'Up next' },
                  { value: 'related', label: 'Related' },
                ]}
              />
              {tab === 'next' && <QueueChips className="mt-2" />}
            </div>
          )}
          <div className="relative min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-3 pb-4">
            {room ? (
              <RoomQueue v={room} />
            ) : tab === 'related' && s.queue.length > 0 ? (
              <RelatedList />
            ) : (
              <>
                <QueueList open={open} />
                <QueueNote className="pt-2" />
              </>
            )}
          </div>
        </motion.aside>
      )}
    </AnimatePresence>
  )
}
