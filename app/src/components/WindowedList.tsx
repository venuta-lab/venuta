import { forwardRef, useCallback, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { scrollerOf } from '../screens/listen/model'

/*
  A long list drawn a screen at a time, inside whatever scrolls around it (a page, a drawer),
  whose rows can be dragged into a new place: the row follows the pointer once it has moved a
  few pixels (so a press is still a click), the others make room, the list scrolls at its
  edges, Escape cancels, and nothing is changed until it is let go. Alt+Up/Down moves a row by
  one from the keyboard. Used by playlists and the queue; it is what keeps a list of thousands
  as quick as a list of ten.
*/

const OVERSCAN = 8
/** How far the pointer moves before a press on a row becomes a drag. */
const DRAG_START = 4

export type Line<R> = { kind: 'row'; key: string; row: R } | { kind: 'head'; key: string; node: ReactNode }

/** What a row needs to be dragged: start on it (or its handle); `dragging` while it is the one moving. */
export interface Grab {
  onGrab: (e: React.PointerEvent) => void
  /** It was just dropped: the click that ends a drag is not a click to play. */
  justDropped: () => boolean
  /** Alt+Up / Alt+Down moves it by one. */
  nudge: (by: -1 | 1) => void
  dragging: boolean
}

export interface WindowedListHandle {
  /** Bring line `i` into view, with `before` pixels of the lines above it still showing. */
  scrollToLine: (i: number, before?: number) => void
}

export interface WindowedListProps<R> {
  lines: Line<R>[]
  rowHeight: number
  headHeight?: number
  /** Which rows can be moved (all, none, or some); rows that can't move get no Grab. */
  movable: boolean | ((i: number) => boolean)
  /** The first and last line a moved row may land on (default: any row). */
  bounds?: [number, number]
  render: (row: R, i: number, style: React.CSSProperties, grab: Grab | null) => ReactNode
  onMove: (from: number, to: number) => void
  label?: string
}

function WindowedListInner<R>(props: WindowedListProps<R>, ref: React.ForwardedRef<WindowedListHandle>) {
  const { lines, rowHeight: ROW } = props
  const HEAD = props.headHeight ?? ROW
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
  }, [lines, ROW, HEAD])
  const height = tops[lines.length]
  const [range, setRange] = useState({ start: 0, end: 30 })
  const { movable } = props
  const canMove = useCallback((i: number) => (typeof movable === 'function' ? movable(i) : movable), [movable])
  const [lo, hi] = props.bounds ?? [0, lines.length - 1]

  // Which lines are on screen, measured against what scrolls around the list.
  const measure = useCallback(() => {
    const el = box.current
    const sc = scrollerOf(el)
    if (!el || !sc) return
    const top = el.getBoundingClientRect().top - sc.getBoundingClientRect().top
    const from = -top
    const to = sc.clientHeight - top
    // Binary search for the first line on screen (lists run to thousands).
    let a = 0
    let b = lines.length
    while (a < b) {
      const m = (a + b) >> 1
      if (tops[m + 1] < from) a = m + 1
      else b = m
    }
    let end = a
    while (end < lines.length && tops[end] < to) end++
    const start = Math.max(0, a - OVERSCAN)
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

  useImperativeHandle(
    ref,
    () => ({
      scrollToLine(i, before = 0) {
        const el = box.current
        const sc = scrollerOf(el)
        if (!el || !sc || i < 0 || i >= lines.length) return
        const listTop = el.getBoundingClientRect().top - sc.getBoundingClientRect().top + sc.scrollTop
        sc.scrollTop = Math.max(0, listTop + tops[i] - before)
        measure()
      },
    }),
    [lines.length, tops, measure],
  )

  // Dragging: which row, where the pointer holds it, and where it would land. A press waits in
  // `pending` until the pointer has moved far enough to mean it.
  const [drag, setDragState] = useState<{ from: number; y: number; over: number } | null>(null)
  const dragNow = useRef(drag)
  const setDrag = (d: typeof drag | ((d: typeof drag) => typeof drag)) => {
    const next = typeof d === 'function' ? d(dragNow.current) : d
    dragNow.current = next
    setDragState(next)
  }
  const pending = useRef<{ i: number; x: number; y: number } | null>(null)
  const dropped = useRef(0)
  const grabAt = useRef(0)
  const pointer = useRef(0)

  const onGrab = (i: number) => (e: React.PointerEvent) => {
    if (!canMove(i) || e.button !== 0) return
    pending.current = { i, x: e.clientX, y: e.clientY }
    const begin = (ev: PointerEvent) => {
      const p = pending.current
      if (!p) return
      if (Math.abs(ev.clientY - p.y) < DRAG_START && Math.abs(ev.clientX - p.x) < DRAG_START) return
      const el = box.current
      if (!el) return
      const listTop = el.getBoundingClientRect().top
      grabAt.current = p.y - listTop - tops[p.i]
      pointer.current = ev.clientY
      pending.current = null
      window.removeEventListener('pointermove', begin)
      // No text selection while a row is carried.
      window.getSelection()?.removeAllRanges()
      setDrag({ from: p.i, y: tops[p.i], over: p.i })
    }
    const release = () => {
      pending.current = null
      window.removeEventListener('pointermove', begin)
    }
    window.addEventListener('pointermove', begin)
    window.addEventListener('pointerup', release, { once: true })
  }

  useEffect(() => {
    if (!drag) return
    const el = box.current
    const sc = scrollerOf(el)
    if (!el || !sc) return
    const place = () => {
      const listTop = el.getBoundingClientRect().top
      const y = Math.max(tops[lo], Math.min(tops[hi], pointer.current - listTop - grabAt.current))
      let over: number
      // Rows only (headings have their own height): the nearest row line at or below y.
      let a = 0
      let b = lines.length - 1
      while (a < b) {
        const m = (a + b + 1) >> 1
        if (tops[m] <= y + ROW / 2) a = m
        else b = m - 1
      }
      over = Math.max(lo, Math.min(hi, a))
      setDrag((d) => (d && (d.y !== y || d.over !== over) ? { ...d, y, over } : d))
    }
    const move = (e: PointerEvent) => {
      pointer.current = e.clientY
      place()
    }
    // Near the top or bottom of what scrolls: scroll, faster the closer.
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
    const cancel = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      e.stopPropagation()
      setDrag(null)
      dropped.current = Date.now()
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up, { once: true })
    window.addEventListener('keydown', cancel, true)
    return () => {
      cancelAnimationFrame(raf)
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      window.removeEventListener('keydown', cancel, true)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [drag?.from, height, lines.length])

  const shown: number[] = []
  for (let i = range.start; i < range.end; i++) shown.push(i)
  if (drag && !shown.includes(drag.from)) shown.push(drag.from)

  return (
    <div ref={box} role="list" aria-label={props.label} className="relative" style={{ height }}>
      {shown.map((i) => {
        const l = lines[i]
        if (!l) return null
        if (l.kind === 'head')
          return (
            <div key={l.key} role="presentation" className="absolute inset-x-0 flex items-end pb-1" style={{ top: tops[i], height: HEAD }}>
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
        const grab: Grab | null = canMove(i)
          ? {
              onGrab: onGrab(i),
              justDropped: () => Date.now() - dropped.current < 250,
              nudge: (by) => {
                const to = i + by
                if (to >= lo && to <= hi && canMove(to)) props.onMove(i, to)
              },
              dragging: moving,
            }
          : null
        return <div key={l.key}>{props.render(l.row, i, style, grab)}</div>
      })}
    </div>
  )
}

export const WindowedList = forwardRef(WindowedListInner) as <R>(p: WindowedListProps<R> & { ref?: React.Ref<WindowedListHandle> }) => ReturnType<typeof WindowedListInner>
