import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useRef, useState } from 'react'
import { useCovers } from '../../lib/covers'
import { clock, jumpTo, move, removeAt, usePlayer, type QueueItem } from '../../lib/player'
import { IconClose } from '../Icons'

const spring = { type: 'spring', stiffness: 380, damping: 36 } as const

function Row(props: {
  item: QueueItem
  index: number
  current: boolean
  playing: boolean
  onDragStart: (i: number) => void
  onDrop: (i: number) => void
}) {
  const cover = useCovers(320)
  const art = cover(props.item.dir)
  const [over, setOver] = useState(false)
  return (
    <li
      draggable
      onDragStart={(e) => {
        e.dataTransfer.effectAllowed = 'move'
        props.onDragStart(props.index)
      }}
      onDragOver={(e) => {
        e.preventDefault()
        setOver(true)
      }}
      onDragLeave={() => setOver(false)}
      onDrop={() => {
        setOver(false)
        props.onDrop(props.index)
      }}
      onDoubleClick={() => void jumpTo(props.index)}
      onKeyDown={(e) => e.key === 'Enter' && void jumpTo(props.index)}
      tabIndex={0}
      className={`group flex min-w-0 cursor-grab items-center gap-3 rounded-xl px-2 py-1.5 active:cursor-grabbing ${
        props.current ? 'bg-ink/[0.08]' : 'hover:bg-ink/[0.05]'
      } ${over ? 'ring-1 ring-[var(--accent)]' : ''}`}
    >
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
          className="grid h-6 w-6 place-items-center rounded-full text-ink/45 opacity-0 hover:bg-ink/10 hover:text-ink group-hover:opacity-100 focus:opacity-100"
        >
          <IconClose width={14} height={14} />
        </button>
      )}
    </li>
  )
}

/** What's playing and what comes next: drag to reorder, double-click to play, × to remove. */
export function QueueDrawer(props: { open: boolean; onClose: () => void }) {
  const s = usePlayer()
  const dragging = useRef<number | null>(null)
  const list = useRef<HTMLDivElement>(null)

  // Bring the playing song into view when the drawer opens.
  useEffect(() => {
    if (!props.open) return
    const t = window.setTimeout(() => {
      const box = list.current
      const el = box?.querySelector<HTMLElement>('[data-current="true"]')
      if (box && el) box.scrollTop = el.offsetTop - box.clientHeight / 2 + el.offsetHeight / 2
    }, 60)
    return () => window.clearTimeout(t)
  }, [props.open])
  const { open, onClose } = props
  useEffect(() => {
    if (!open) return
    const esc = (e: KeyboardEvent) => e.key === 'Escape' && onClose()
    window.addEventListener('keydown', esc)
    return () => window.removeEventListener('keydown', esc)
  }, [open, onClose])

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
              <h2 className="text-[18px] font-semibold tracking-[-0.01em]">Queue</h2>
              <p className="mt-0.5 truncate text-[12px] text-ink/55">
                {s.from ? `Playing from ${s.from}. ` : ''}
                {upcoming > 0 ? `${upcoming} up next, ${clock(left)}` : 'Nothing after this song'}
              </p>
            </div>
            <button aria-label="Close the queue" onClick={props.onClose} className="grid h-8 w-8 place-items-center rounded-full text-ink/60 hover:bg-ink/10 hover:text-ink">
              <IconClose />
            </button>
          </header>
          <div ref={list} className="relative min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-3 pb-4">
            {s.queue.length === 0 ? (
              <p className="px-2 py-10 text-center text-[13px] text-ink/55">Play an album or a song, and it lines up here.</p>
            ) : (
              <ol className="grid gap-0.5">
                {s.queue.map((q, i) => (
                  <div key={`${q.path}#${i}`} data-current={i === s.index} className="min-w-0">
                    {i === s.index + 1 && <div className="px-2 pb-1 pt-3 text-[12px] text-ink/50">Up next</div>}
                    <Row
                      item={q}
                      index={i}
                      current={i === s.index}
                      playing={s.playing}
                      onDragStart={(k) => (dragging.current = k)}
                      onDrop={(k) => {
                        if (dragging.current !== null && dragging.current !== k) void move(dragging.current, k)
                        dragging.current = null
                      }}
                    />
                  </div>
                ))}
              </ol>
            )}
          </div>
        </motion.aside>
      )}
    </AnimatePresence>
  )
}
