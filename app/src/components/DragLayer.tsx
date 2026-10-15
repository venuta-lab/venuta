import { targetLabel, useDrag } from '../lib/drag'

/*
  What is being dragged, following the pointer: its picture, what it is, and what letting go
  will do here ("Add to Road trip", "Play next"), or where it can go. Put once, at the root.
*/
export function DragLayer() {
  const d = useDrag()
  if (!d) return null
  const what = d.carried.count && d.carried.count > 1 ? `${d.carried.label}, ${d.carried.count.toLocaleString('en-IN')} songs` : d.carried.label
  const here = targetLabel(d.over)
  return (
    <div
      aria-hidden
      className="pointer-events-none fixed left-0 top-0 z-[70]"
      style={{ transform: `translate(${d.x + 14}px, ${d.y + 14}px)` }}
    >
      <div className="popover flex max-w-[300px] items-center gap-3 rounded-2xl p-2 pr-4 shadow-2xl shadow-black/40">
        <div className="grid h-10 w-10 shrink-0 place-items-center overflow-hidden rounded-lg bg-ink/[0.08] text-ink/40">
          {d.carried.cover ? <img src={d.carried.cover} alt="" className="h-full w-full object-cover" draggable={false} /> : '♪'}
        </div>
        <div className="min-w-0">
          <div className="truncate text-[13px] font-medium">{what}</div>
          <div className={`truncate text-[12px] ${here ? 'text-accent-ink' : 'text-ink/50'}`}>
            {here ?? 'Drop on a playlist, the queue or the player'}
          </div>
        </div>
      </div>
    </div>
  )
}
