import { askAddToPlaylist } from '../AddToPlaylist'
import { useEscape } from '../../lib/layers'
import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useMemo, useRef } from 'react'
import { useCovers } from '../../lib/covers'
import { clock, jumpTo, move, removeAt, toggle, usePlayer, type QueueItem } from '../../lib/player'
import { IconClose } from '../Icons'
import { PlayingFrom } from './PlayingFrom'
import { WindowedList, type Grab, type Line, type WindowedListHandle } from '../WindowedList'

const spring = { type: 'spring', stiffness: 380, damping: 36 } as const

/** One row's height; the "Up next" heading's. */
const ROW_H = 50
const HEAD_H = 34

/** A song in the queue as the list shows it: the song and its place in the queue. */
type Row = { q: QueueItem; at: number }

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

const ROW = 'group absolute inset-x-0 flex min-w-0 cursor-pointer items-center gap-3 rounded-xl px-2'

/**
 * What's playing and what comes next: click to play, drag (or Alt+↑/↓) to reorder, × or Delete
 * to remove. Drawn a screen at a time, so a queue of the whole library opens as fast as an album.
 */
export function QueueDrawer(props: { open: boolean; onClose: () => void }) {
  const s = usePlayer()
  const list = useRef<WindowedListHandle>(null)
  const { open, onClose } = props
  useEscape(open, onClose)

  // The player keeps the same queue until it changes, so this is rebuilt only then (not on
  // every position update).
  const { lines, firstNext } = useMemo(() => {
    const out: Line<Row>[] = []
    s.queue.forEach((q, at) => {
      if (at === s.index + 1) out.push({ kind: 'head', key: 'up-next', node: <div className="px-2 text-[12px] text-ink/50">Up next</div> })
      out.push({ kind: 'row', key: `${q.path}#${at}`, row: { q, at } })
    })
    return { lines: out, firstNext: s.index + 2 }
  }, [s.queue, s.index])

  // Bring the playing song into view when the drawer opens, with the one before it in sight.
  useEffect(() => {
    if (!open) return
    const t = window.setTimeout(() => list.current?.scrollToLine(Math.max(0, s.index), ROW_H + 8), 30)
    return () => window.clearTimeout(t)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open])

  const upcoming = s.queue.length - s.index - 1
  const left = useMemo(() => s.queue.slice(s.index + 1).reduce((t, q) => t + q.duration_ms, 0), [s.queue, s.index])

  const row = (r: Row, _line: number, style: React.CSSProperties, grab: Grab | null) => {
    const current = r.at === s.index
    const played = r.at < s.index
    return (
      <div
        role="listitem"
        tabIndex={0}
        style={style}
        data-current={current}
        onPointerDown={grab?.onGrab}
        onClick={(e) => {
          if ((e.target as HTMLElement).closest('button') || grab?.justDropped()) return
          void (current ? toggle() : jumpTo(r.at))
        }}
        onKeyDown={(e) => {
          if (e.target !== e.currentTarget) return
          if (e.key === 'Enter') void (current ? toggle() : jumpTo(r.at))
          // Alt+arrows move a song without dragging.
          if (grab && e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
            e.preventDefault()
            grab.nudge(e.key === 'ArrowUp' ? -1 : 1)
          }
          if (e.key === 'Delete' && !current) void removeAt(r.at)
        }}
        title={grab ? 'Drag to move; Alt+↑/↓ with the keyboard' : undefined}
        className={`${ROW} ${
          current
            ? 'bg-ink/[0.08]'
            : grab?.dragging
              ? 'scale-[1.02] bg-[var(--popover)] shadow-[0_12px_28px_-12px_rgb(0_0_0/0.55)] active:cursor-grabbing'
              : `hover:bg-ink/[0.05] active:bg-ink/[0.09] ${played ? 'opacity-70' : ''}`
        }`}
      >
        <RowBody item={r.q} index={r.at} current={current} />
      </div>
    )
  }

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
              <PlayingFrom className="mt-0.5 block truncate text-[12px] text-ink/55" />
              <p className="mt-0.5 truncate text-[12px] text-ink/55">
                {upcoming > 0 ? `${upcoming.toLocaleString('en-IN')} up next, ${clock(left)}` : 'Nothing after this song'}
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
          <div className="relative min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-3 pb-4">
            {s.queue.length === 0 ? (
              <p className="px-2 py-10 text-center text-[13px] text-ink/55">Play an album or a song, and it lines up here.</p>
            ) : (
              <WindowedList
                ref={list}
                label="Songs in the queue"
                lines={lines}
                rowHeight={ROW_H}
                headHeight={HEAD_H}
                // Only what comes next can move, and only among what comes next.
                movable={(i) => i >= firstNext}
                bounds={[firstNext, lines.length - 1]}
                onMove={(from, to) => {
                  const a = lines[from]
                  const b = lines[to]
                  if (a?.kind === 'row' && b?.kind === 'row') void move(a.row.at, b.row.at)
                }}
                render={row}
              />
            )}
          </div>
        </motion.aside>
      )}
    </AnimatePresence>
  )
}
