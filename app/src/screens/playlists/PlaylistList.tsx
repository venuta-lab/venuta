import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { scrollerOf } from '../listen/model'

/** One row's height, and an album heading's (the To get view groups by album). */
export const ROW = 64
const HEAD = 52
const OVERSCAN = 8

export type Line<R> = { kind: 'row'; key: string; row: R } | { kind: 'head'; key: string; node: ReactNode }

/** What a row needs to be dragged: start on its handle; `dragging` while it is the one moving. */
export interface Grab {
  onGrab: (e: React.PointerEvent) => void
  /** It was just dropped: the click that ends a drag is not a click to play. */
  justDropped: () => boolean
  /** Alt+Up / Alt+Down moves it by one. */
  nudge: (by: -1 | 1) => void
  dragging: boolean
}

/**
 * A long list drawn a screen at a time (playlists run to hundreds of songs), with rows that can
 * be dragged into a new place: the row follows the pointer, the others make room, the screen
 * scrolls at its edges, and nothing is saved until it is let go.
 */
export function PlaylistList<R>(props: {
  lines: Line<R>[]
  /** Rows can be moved (only when every row of the playlist is shown, in its order). */
  movable: boolean
  render: (row: R, i: number, style: React.CSSProperties, grab: Grab | null) => ReactNode
  onMove: (from: number, to: number) => void
}) {
  const { lines } = props
  const box = useRef<HTMLDivElement>(null)
  const tops = useMemo(() => {
    const out: number[] = []
    let y = 0
    for (const l of lines) {
      out.push(y)
      y += l.kind === 'row' ? ROW : HEAD
    }
    out.push(y)
    return out
  }, [lines])
  const height = tops[lines.length]
  const [range, setRange] = useState({ start: 0, end: 30 })

  // Which lines are on screen, measured against the page's own scroll area.
  const measure = useCallback(() => {
    const el = box.current
    const sc = scrollerOf(el)
    if (!el || !sc) return
    const top = el.getBoundingClientRect().top - sc.getBoundingClientRect().top
    const from = -top
    const to = sc.clientHeight - top
    let start = 0
    while (start < lines.length && tops[start + 1] < from) start++
    let end = start
    while (end < lines.length && tops[end] < to) end++
    start = Math.max(0, start - OVERSCAN)
    end = Math.min(lines.length, end + OVERSCAN)
    setRange((r) => (r.start === start && r.end === end ? r : { start, end }))
  }, [lines.length, tops])
  useLayoutEffect(() => {
    const sc = scrollerOf(box.current)
    if (!sc) return
    measure()
    sc.addEventListener('scroll', measure, { passive: true })
    const ro = new ResizeObserver(measure)
    ro.observe(sc)
    return () => {
      sc.removeEventListener('scroll', measure)
      ro.disconnect()
    }
  }, [measure])

  // Dragging: which row, where the pointer holds it, and where it would land.
  const [drag, setDragState] = useState<{ from: number; y: number; over: number } | null>(null)
  const dragNow = useRef(drag)
  const setDrag = (d: typeof drag | ((d: typeof drag) => typeof drag)) => {
    const next = typeof d === 'function' ? d(dragNow.current) : d
    dragNow.current = next
    setDragState(next)
  }
  const dropped = useRef(0)
  const grabAt = useRef(0)
  const pointer = useRef(0)

  const onGrab = (i: number) => (e: React.PointerEvent) => {
    if (!props.movable || e.button !== 0) return
    e.preventDefault()
    const el = box.current
    if (!el) return
    const listTop = el.getBoundingClientRect().top
    grabAt.current = e.clientY - listTop - tops[i]
    pointer.current = e.clientY
    setDrag({ from: i, y: tops[i], over: i })
  }

  useEffect(() => {
    if (!drag) return
    const el = box.current
    const sc = scrollerOf(el)
    if (!el || !sc) return
    const place = () => {
      const listTop = el.getBoundingClientRect().top
      const y = Math.max(0, Math.min(height - ROW, pointer.current - listTop - grabAt.current))
      const over = Math.max(0, Math.min(lines.length - 1, Math.round(y / ROW)))
      setDrag((d) => (d && (d.y !== y || d.over !== over) ? { ...d, y, over } : d))
    }
    const move = (e: PointerEvent) => {
      pointer.current = e.clientY
      place()
    }
    // Near the top or bottom of the screen: scroll, faster the closer.
    let raf = 0
    const edge = () => {
      const r = sc.getBoundingClientRect()
      const near = 72
      const up = pointer.current - r.top
      const down = r.bottom - pointer.current
      const speed = up < near ? -((near - up) / near) * 18 : down < near ? ((near - down) / near) * 18 : 0
      if (speed) {
        sc.scrollTop += speed
        place()
      }
      raf = requestAnimationFrame(edge)
    }
    raf = requestAnimationFrame(edge)
    const up = () => {
      const d = dragNow.current
      setDrag(null)
      dropped.current = Date.now()
      if (d && d.over !== d.from) props.onMove(d.from, d.over)
    }
    const cancel = (e: KeyboardEvent) => e.key === 'Escape' && setDrag(null)
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up, { once: true })
    window.addEventListener('keydown', cancel)
    return () => {
      cancelAnimationFrame(raf)
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      window.removeEventListener('keydown', cancel)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [drag?.from, height, lines.length])

  const shown: number[] = []
  for (let i = range.start; i < range.end; i++) shown.push(i)
  if (drag && !shown.includes(drag.from)) shown.push(drag.from)

  return (
    <div ref={box} role="list" className="relative" style={{ height }}>
      {shown.map((i) => {
        const l = lines[i]
        if (!l) return null
        if (l.kind === 'head')
          return (
            <div key={l.key} className="absolute inset-x-0 flex items-end pb-1" style={{ top: tops[i], height: HEAD }}>
              {l.node}
            </div>
          )
        const moving = drag?.from === i
        // The others make room for the one moving.
        let top = tops[i]
        if (drag && !moving) {
          if (drag.from < i && i <= drag.over) top -= ROW
          else if (drag.over <= i && i < drag.from) top += ROW
        }
        const style: React.CSSProperties = moving
          ? { top: drag.y, height: ROW, zIndex: 20 }
          : { top, height: ROW, transition: drag ? 'top 160ms cubic-bezier(.2,.8,.2,1)' : undefined }
        const grab: Grab | null = props.movable
          ? {
              onGrab: onGrab(i),
              justDropped: () => Date.now() - dropped.current < 250,
              nudge: (by) => {
                const to = i + by
                if (to >= 0 && to < lines.length) props.onMove(i, to)
              },
              dragging: moving,
            }
          : null
        return <div key={l.key}>{props.render(l.row, i, style, grab)}</div>
      })}
    </div>
  )
}
