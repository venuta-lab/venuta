import { useCallback, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { scrollerOf } from '../screens/listen/model'

/*
  A grid of cards drawn a few rows at a time, inside whatever scrolls around it. It lays out
  exactly as `grid-template-columns: repeat(auto-fill, minmax(<min>px, 1fr))` would, and takes
  its row height from a card as drawn (so a theme that pads or restyles cards still lines up).
  A card that has focus stays drawn while it is scrolled away, so the keyboard never loses it.
*/

const OVERSCAN_ROWS = 2

export function WindowedGrid<T>(props: {
  items: T[]
  keyOf: (item: T) => string
  /** The narrowest a column may be, as in minmax(min, 1fr). */
  min: number
  gapX: number
  gapY: number
  /** A first guess at a card's height below its square art, until one is measured. */
  below: number
  render: (item: T, i: number) => ReactNode
  label?: string
}) {
  const { items, min, gapX, gapY } = props
  const box = useRef<HTMLDivElement>(null)
  const [width, setWidth] = useState(0)
  const [measured, setMeasured] = useState(0)
  const [rows, setRows] = useState({ start: 0, end: 4 })
  const [focused, setFocused] = useState(-1)

  const cols = width ? Math.max(1, Math.floor((width + gapX) / (min + gapX))) : 1
  const cell = width ? (width - gapX * (cols - 1)) / cols : min
  const rowH = measured || cell + props.below
  const count = Math.ceil(items.length / cols)
  const height = count ? count * (rowH + gapY) - gapY : 0

  const measure = useCallback(() => {
    const el = box.current
    const sc = scrollerOf(el)
    if (!el || !sc) return
    const top = el.getBoundingClientRect().top - sc.getBoundingClientRect().top
    const step = rowH + gapY
    const start = Math.max(0, Math.floor(-top / step) - OVERSCAN_ROWS)
    const end = Math.min(count, Math.ceil((sc.clientHeight - top) / step) + OVERSCAN_ROWS)
    setRows((r) => (r.start === start && r.end === end ? r : { start, end }))
  }, [rowH, gapY, count])

  useLayoutEffect(() => {
    const el = box.current
    const sc = scrollerOf(el)
    if (!el || !sc) return
    setWidth(el.clientWidth)
    measure()
    sc.addEventListener('scroll', measure, { passive: true })
    const ro = new ResizeObserver(() => {
      setWidth(el.clientWidth)
      measure()
    })
    ro.observe(el)
    ro.observe(sc)
    return () => {
      sc.removeEventListener('scroll', measure)
      ro.disconnect()
    }
  }, [measure])

  // A card's real height, from the first one drawn, and again whenever it changes size (a new
  // width, a theme or a font that restyles cards).
  useLayoutEffect(() => {
    const first = box.current?.querySelector<HTMLElement>('[data-cell]')
    if (!first) return
    const read = () => {
      const h = first.offsetHeight
      if (h) setMeasured((m) => (Math.abs(h - m) > 0.5 ? h : m))
    }
    read()
    const ro = new ResizeObserver(read)
    ro.observe(first)
    return () => ro.disconnect()
  }, [rows.start, cols, items])

  const shown: number[] = []
  for (let r = rows.start; r < rows.end; r++) shown.push(r)
  const focusRow = focused >= 0 ? Math.floor(focused / cols) : -1
  if (focusRow >= 0 && focusRow < count && !shown.includes(focusRow)) shown.push(focusRow)

  return (
    <div
      ref={box}
      role="list"
      aria-label={props.label}
      className="relative"
      style={{ height }}
      onFocus={(e) => {
        const at = (e.target as HTMLElement).closest<HTMLElement>('[data-cell]')?.dataset.cell
        if (at !== undefined) setFocused(Number(at))
      }}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setFocused(-1)
      }}
    >
      {shown.map((r) => (
        <div
          key={r}
          className="absolute inset-x-0 grid"
          style={{ top: r * (rowH + gapY), gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))`, columnGap: gapX }}
        >
          {items.slice(r * cols, r * cols + cols).map((item, k) => {
            const i = r * cols + k
            return (
              <div key={props.keyOf(item)} role="listitem" data-cell={i} className="min-w-0">
                {props.render(item, i)}
              </div>
            )
          })}
        </div>
      ))}
    </div>
  )
}
