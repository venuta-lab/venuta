import { useMemo } from 'react'
import { songLength } from '../../lib/player'
import { can, jumpInRoom, memberName, moveInRoom, removeFromRoom, type RoomSong, type TogetherView } from '../../lib/together'
import { DragHandle, MOVE_KEYS } from '../DragHandle'
import { IconClose } from '../Icons'
import { RemoteArt } from '../RemoteArt'
import { WindowedList, type Grab, type Line } from '../WindowedList'

/*
  The queue drawer while you're in a room: the room's queue, the same for everyone, with who
  added each song. Whoever may control the room jumps, moves and takes out songs here; the
  others see it.
*/

const ROW_H = 50
const HEAD_H = 34
const ROW = 'group absolute inset-x-0 flex min-w-0 items-center gap-3 rounded-xl px-2'

type Row = { song: RoomSong; at: number }

const NONE: RoomSong[] = []

export function RoomQueue({ v }: { v: TogetherView }) {
  const q = v.room?.queue ?? NONE
  const at = v.room?.timeline?.index ?? -1
  const control = can(v, 'control')
  const { lines, firstNext } = useMemo(() => {
    const out: Line<Row>[] = []
    q.forEach((song, i) => {
      if (i === at + 1) out.push({ kind: 'head', key: 'up-next', node: <div className="px-2 text-[12px] text-ink/50">Up next</div> })
      out.push({ kind: 'row', key: `${song.key}#${i}`, row: { song, at: i } })
    })
    return { lines: out, firstNext: at + 2 }
  }, [q, at])

  if (!q.length) return <p className="px-2 py-10 text-center text-[13px] text-ink/55">The room's queue is empty. Play or queue a song and it lines up here for everyone.</p>

  const row = (r: Row, _line: number, style: React.CSSProperties, grab: Grab | null) => {
    const current = r.at === at
    const by = r.song.added_by != null ? memberName(v, r.song.added_by) : ''
    return (
      <div
        role="listitem"
        aria-keyshortcuts={grab ? MOVE_KEYS : undefined}
        tabIndex={0}
        style={style}
        data-current={current}
        onPointerDown={grab?.onGrab}
        onClick={(e) => {
          if ((e.target as HTMLElement).closest('button') || grab?.justDropped() || current) return
          void jumpInRoom(r.at)
        }}
        onKeyDown={(e) => {
          if (e.target !== e.currentTarget) return
          if (e.key === 'Enter' && !current) void jumpInRoom(r.at)
          if (grab && e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
            e.preventDefault()
            grab.nudge(e.key === 'ArrowUp' ? -1 : 1)
          }
          if (e.key === 'Delete' && !current && control) void removeFromRoom(r.at)
        }}
        title={grab ? 'Drag to move; Alt+↑/↓ with the keyboard' : undefined}
        className={`${ROW} ${control && !current ? 'cursor-pointer' : ''} ${
          current
            ? 'bg-ink/[0.08]'
            : grab?.dragging
              ? 'scale-[1.02] bg-[var(--popover)] shadow-[0_12px_28px_-12px_rgb(0_0_0/0.55)] active:cursor-grabbing'
              : `hover:bg-ink/[0.05] ${r.at < at ? 'opacity-70' : ''}`
        }`}
      >
        {grab ? <DragHandle label={r.song.title} dragging={grab.dragging} /> : <span aria-hidden className="w-3 shrink-0" />}
        <div className="grid h-9 w-9 shrink-0 place-items-center overflow-hidden rounded-lg bg-ink/[0.08] text-ink/30">
          <RemoteArt url={r.song.artwork_url ?? undefined} seed={r.song.album || r.song.title} size={36} imgClassName="h-full w-full object-cover" />
        </div>
        <div className="min-w-0 flex-1">
          <div className={`truncate text-[13px] ${current ? 'font-medium text-accent-ink' : ''}`}>{r.song.title}</div>
          <div className="truncate text-[11.5px] text-ink/55">
            {r.song.artists.join(', ')}
            {by && <span className="text-ink/40">{` · added by ${by}`}</span>}
          </div>
        </div>
        <span className="text-[11.5px] tabular-nums text-ink/45">{songLength(r.song.duration_ms ?? 0)}</span>
        {control && !current && (
          <button
            aria-label={`Take ${r.song.title} out of the room's queue`}
            onClick={() => void removeFromRoom(r.at)}
            className="grid h-6 w-6 place-items-center rounded-full text-ink/45 opacity-0 hover:bg-ink/10 hover:text-ink focus:opacity-100 group-hover:opacity-100"
          >
            <IconClose width={14} height={14} />
          </button>
        )}
      </div>
    )
  }

  return (
    <WindowedList
      label="Songs in the room's queue"
      lines={lines}
      rowHeight={ROW_H}
      headHeight={HEAD_H}
      movable={(i) => control && i >= firstNext}
      bounds={[firstNext, lines.length - 1]}
      onMove={(from, to) => {
        const a = lines[from]
        const b = lines[to]
        if (a?.kind === 'row' && b?.kind === 'row') void moveInRoom(a.row.at, b.row.at)
      }}
      render={row}
    />
  )
}
