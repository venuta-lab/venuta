import { askAddToPlaylist } from '../AddToPlaylist'
import { useEscape } from '../../lib/layers'
import { AnimatePresence, motion, Reorder } from 'motion/react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useCovers } from '../../lib/covers'
import { clock, jumpTo, move, removeAt, toggle, usePlayer, type QueueItem } from '../../lib/player'
import { IconClose } from '../Icons'
import { PlayingFrom } from './PlayingFrom'

const spring = { type: 'spring', stiffness: 380, damping: 36 } as const

type Entry = { q: QueueItem; i: number; key: string }

/** One song in the queue: click plays it (the one playing now pauses or resumes). */
function RowBody(props: { item: QueueItem; index: number; current: boolean }) {
  const cover = useCovers(320)
  const art = cover(props.item.dir)
  return (
    <>
      <div className="h-9 w-9 shrink-0 overflow-hidden rounded-lg bg-ink/[0.08]">
        {art && <img src={art.src} alt="" className="h-full w-full object-cover" draggable={false} />}
      </div>
      <div className="min-w-0 flex-1">
        <div className={`truncate text-[13px] ${props.current ? 'font-medium text-accent-ink' : ''}`}>{props.item.title}</div>
        <div className="truncate text-[11.5px] text-ink/55">{props.item.artists.join(', ') || props.item.album_artist}</div>
      </div>
      <span className="text-[11.5px] tabular-nums text-ink/45">{clock(props.item.duration_ms)}</span>
      {!props.current && (
        <button
          aria-label={`Remove ${props.item.title} from the queue`}
          onClick={() => void removeAt(props.index)}
          className="grid h-6 w-6 place-items-center rounded-full text-ink/45 opacity-0 hover:bg-ink/10 hover:text-ink focus:opacity-100 group-hover:opacity-100"
        >
          <IconClose width={14} height={14} />
        </button>
      )}
    </>
  )
}

const ROW = 'group flex min-w-0 cursor-pointer items-center gap-3 rounded-xl px-2 py-1.5'

function play(e: React.MouseEvent, index: number, current: boolean) {
  if ((e.target as HTMLElement).closest('button')) return
  void (current ? toggle() : jumpTo(index))
}

/** What's playing and what comes next: click to play, drag to reorder, × to remove. */
export function QueueDrawer(props: { open: boolean; onClose: () => void }) {
  const s = usePlayer()
  const list = useRef<HTMLDivElement>(null)

  // Bring the playing song into view when the drawer opens.
  useEffect(() => {
    if (!props.open) return
    const t = window.setTimeout(() => {
      const box = list.current
      const el = box?.querySelector<HTMLElement>('[data-current="true"]')
      // Near the top, with the song before it still in sight: the drawer is about what comes next.
      if (box && el) box.scrollTop = Math.max(0, el.offsetTop - el.offsetHeight - 8)
    }, 60)
    return () => window.clearTimeout(t)
  }, [props.open])
  const { open, onClose } = props
  useEscape(open, onClose)

  // "Up next" in the order shown; it follows the player except while you drag.
  const sig = `${s.index}|${s.queue.map((q) => q.path).join('\n')}`
  const after = useMemo<Entry[]>(
    () => s.queue.slice(s.index + 1).map((q, k) => ({ q, i: s.index + 1 + k, key: `${q.path}#${s.index + 1 + k}` })),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [sig],
  )
  const [order, setOrder] = useState(after)
  useEffect(() => setOrder(after), [after])
  const dragged = useRef(false)

  /** Where the dragged song landed, sent to the player as one move. */
  function dropped(e: Entry) {
    const to = s.index + 1 + order.findIndex((x) => x.key === e.key)
    if (to !== e.i) void move(e.i, to)
    window.setTimeout(() => (dragged.current = false), 0)
  }

  const upcoming = s.queue.length - s.index - 1
  const left = s.queue.slice(s.index + 1).reduce((t, q) => t + q.duration_ms, 0)
  return (
    <AnimatePresence>
      {props.open && (
        <motion.aside
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
              <p className="mt-0.5 truncate text-[12px] text-ink/55">
                <PlayingFrom />
                {s.from ? '. ' : ''}
                {upcoming > 0 ? `${upcoming} up next, ${clock(left)}` : 'Nothing after this song'}
              </p>
            </div>
            {s.queue.length > 0 && (
              <button
                onClick={() => askAddToPlaylist(s.queue.map((q) => q.path), 'the queue')}
                className="btn-ghost btn-sm mt-0.5 shrink-0"
                title="Save every song in the queue as a playlist, or add them to one"
              >
                Save as playlist
              </button>
            )}
            <button aria-label="Close the queue" onClick={props.onClose} className="grid h-8 w-8 place-items-center rounded-full text-ink/60 hover:bg-ink/10 hover:text-ink">
              <IconClose />
            </button>
          </header>
          <div ref={list} className="relative min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-3 pb-4">
            {s.queue.length === 0 ? (
              <p className="px-2 py-10 text-center text-[13px] text-ink/55">Play an album or a song, and it lines up here.</p>
            ) : (
              <>
                <ol className="grid gap-0.5">
                  {s.queue.slice(0, s.index + 1).map((q, i) => (
                    <li
                      key={`${q.path}#${i}`}
                      data-current={i === s.index}
                      tabIndex={0}
                      onClick={(e) => play(e, i, i === s.index)}
                      onKeyDown={(e) => e.key === 'Enter' && e.target === e.currentTarget && void (i === s.index ? toggle() : jumpTo(i))}
                      className={`${ROW} ${i === s.index ? 'bg-ink/[0.08]' : 'hover:bg-ink/[0.05] active:bg-ink/[0.09]'}`}
                    >
                      <RowBody item={q} index={i} current={i === s.index} />
                    </li>
                  ))}
                </ol>
                {order.length > 0 && (
                  <>
                    <div className="px-2 pb-1 pt-3 text-[12px] text-ink/50">Up next</div>
                    <Reorder.Group as="ol" axis="y" values={order} onReorder={setOrder} className="grid gap-0.5">
                      {order.map((e) => (
                        <Reorder.Item
                          key={e.key}
                          value={e}
                          as="li"
                          tabIndex={0}
                          onDragStart={() => (dragged.current = true)}
                          onDragEnd={() => dropped(e)}
                          onClick={(ev) => !dragged.current && play(ev, e.i, false)}
                          onKeyDown={(ev) => {
                            if (ev.target !== ev.currentTarget) return
                            if (ev.key === 'Enter') void jumpTo(e.i)
                            // Alt+arrows move a song without dragging.
                            if (ev.altKey && (ev.key === 'ArrowUp' || ev.key === 'ArrowDown')) {
                              ev.preventDefault()
                              const to = e.i + (ev.key === 'ArrowUp' ? -1 : 1)
                              if (to > s.index && to < s.queue.length) void move(e.i, to)
                            }
                            if (ev.key === 'Delete') void removeAt(e.i)
                          }}
                          whileDrag={{ scale: 1.02, boxShadow: '0 12px 28px -12px rgb(0 0 0 / 0.55)', backgroundColor: 'var(--popover)' }}
                          title="Drag to move; Alt+↑/↓ with the keyboard"
                          className={`${ROW} relative hover:bg-ink/[0.05] active:cursor-grabbing active:bg-ink/[0.09]`}
                        >
                          <RowBody item={e.q} index={e.i} current={false} />
                        </Reorder.Item>
                      ))}
                    </Reorder.Group>
                  </>
                )}
              </>
            )}
          </div>
        </motion.aside>
      )}
    </AnimatePresence>
  )
}
