/*
  The grip on a row that can be moved (a playlist's songs, what comes next in the queue): always
  there, faint until the row is hovered, so it is clear the row moves. Its row says the keys
  (aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown", see MOVE_KEYS) for the keyboard.
*/

/** For the row: the keys that move it, said to screen readers. */
export const MOVE_KEYS = 'Alt+ArrowUp Alt+ArrowDown'

export function DragHandle(props: { label: string; onPointerDown?: (e: React.PointerEvent) => void; dragging?: boolean }) {
  return (
    <span
      role="img"
      aria-label={`Move ${props.label}: drag, or Alt+Up and Alt+Down`}
      title="Drag to move (or Alt+Up, Alt+Down)"
      onPointerDown={props.onPointerDown}
      className={`grid h-8 cursor-grab touch-none place-items-center transition-colors active:cursor-grabbing ${
        props.dragging ? 'text-ink/70' : 'text-ink/25 hover:text-ink/70 group-hover:text-ink/45'
      }`}
    >
      <svg width="12" height="16" viewBox="0 0 12 16" fill="currentColor" aria-hidden>
        {[2, 7, 12].map((y) => (
          <g key={y}>
            <circle cx="3" cy={y} r="1.3" />
            <circle cx="9" cy={y} r="1.3" />
          </g>
        ))}
      </svg>
    </span>
  )
}
