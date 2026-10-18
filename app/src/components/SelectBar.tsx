import { AnimatePresence, motion } from 'motion/react'
import { useEscape } from '../lib/layers'
import { askAddSongs } from './AddToPlaylist'
import { IconHeart, IconPlay, IconPlaylistAdd } from './Icons'
import { likeMany } from '../lib/likes'
import { enqueueItems, playItems, songItemOf, type From, type QueueItem } from '../lib/player'

/*
  The bar that floats over a page while songs are being picked ("Select" on an album or a
  playlist): how many, what can be done with them all at once, Select all, and Done. Esc
  leaves picking. It sits above the player bar, on the theme's floating surface.
*/

const spring = { type: 'spring', stiffness: 520, damping: 38 } as const

export function SelectBar(props: {
  open: boolean
  root: string
  /** The picked songs that play, in the list's order. */
  items: QueueItem[]
  /** How many are picked, when some cannot play (songs still to get); else `items`. */
  count?: number
  total: number
  /** What only this list can do with them (Remove from playlist). */
  more?: { label: string; run: () => void }[]
  /** Where playing them says it played from. */
  from: From
  label: string
  onAll: () => void
  onDone: () => void
}) {
  useEscape(props.open, props.onDone)
  const n = props.items.length
  const picked = props.count ?? n
  const all = picked > 0 && picked === props.total
  const act = (run: () => void) => () => {
    run()
    props.onDone()
  }
  const button = 'btn-ghost btn-sm h-9 shrink-0 gap-1.5 disabled:opacity-40'
  return (
    <AnimatePresence>
      {props.open && (
        <motion.div
          role="toolbar"
          aria-label="Picked songs"
          initial={{ opacity: 0, y: 24, scale: 0.98 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={{ opacity: 0, y: 16, scale: 0.98 }}
          transition={spring}
          className="popover fixed inset-x-0 bottom-[116px] z-40 mx-auto flex w-fit max-w-[calc(100vw-32px)] items-center gap-1 overflow-x-auto rounded-full py-1.5 pl-5 pr-1.5 no-scrollbar"
        >
          <span className="mr-2 shrink-0 text-[13px] font-medium tabular-nums" aria-live="polite">
            {picked ? `${picked} picked` : 'Pick songs to act on them'}
          </span>
          <button disabled={!n} className={button} onClick={act(() => void playItems(props.root, props.items, 0, props.from))}>
            <IconPlay width={14} height={14} />
            Play
          </button>
          <button disabled={!n} className={button} onClick={act(() => void enqueueItems(props.root, props.items, true))}>
            Play next
          </button>
          <button disabled={!n} className={button} onClick={act(() => void enqueueItems(props.root, props.items, false))}>
            Add to queue
          </button>
          <button disabled={!n} className={button} onClick={act(() => void likeMany(props.root, props.items))}>
            <IconHeart width={15} height={15} />
            Like
          </button>
          <button disabled={!n} className={button} onClick={act(() => askAddSongs(props.items.map(songItemOf), n === 1 ? props.items[0].title : props.label))}>
            <IconPlaylistAdd width={15} height={15} />
            Add to playlist
          </button>
          {props.more?.map((m) => (
            <button key={m.label} disabled={!picked} className={button} onClick={m.run}>
              {m.label}
            </button>
          ))}
          <span aria-hidden className="mx-1 h-5 w-px shrink-0 bg-ink/15" />
          <button className={button} onClick={props.onAll}>
            {all ? 'Select none' : `Select all (${props.total})`}
          </button>
          <button className="btn-primary btn-sm h-9 shrink-0" onClick={props.onDone}>
            Done
          </button>
        </motion.div>
      )}
    </AnimatePresence>
  )
}

/** A row's tick while picking: an empty ring, or the accent with a tick when picked. */
export function PickMark({ picked }: { picked: boolean }) {
  return (
    <span
      aria-hidden
      className={`grid h-5 w-5 place-items-center rounded-full border-[1.5px] transition-colors duration-150 ${picked ? 'border-accent bg-accent text-on-accent' : 'border-ink/35 group-hover:border-ink/60'}`}
    >
      {picked && (
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
          <path d="m5 12 5 5 9-10" />
        </svg>
      )}
    </span>
  )
}
