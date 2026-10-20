import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type HTMLAttributes, type ReactNode } from 'react'
import { IconChevron } from './Icons'
import { GLIDE_MS, glideAt, pageBy, revealLeft, rowEnds, rowWheel, scrollTarget, stepItem } from '../lib/hscroll'

/*
  Every row that scrolls sideways (shelves of cards, rows of chips), so they all behave the
  same (what decides is lib/hscroll.ts):
  - a click of a sideways wheel (the tilt wheel, Shift and the wheel) glides the row, a card a
    click on a row of cards; a touchpad scrolls it the browser's own way; up and down stays the
    page's;
  - its edges fade where there is more (`fade`), and on a row of cards arrows show on hover
    (`arrows`) that move by most of what shows;
  - Left and Right move between its items once one has focus (Tab reaches each, as before);
  - it never scrolls the page sideways, and shows no scrollbar.
*/

/** The edge fade of a row of cards: within the gutter the row leaves for hover lifts, so only the
 *  cut-off card at an edge softens. */
export const CARD_FADE = 12
/** The edge fade of a row of chips. */
export const CHIP_FADE = 24

type RowProps = Omit<HTMLAttributes<HTMLDivElement>, 'onScroll' | 'className' | 'style' | 'children'>

export function ScrollRow({
  children,
  className = '',
  style,
  fade = 0,
  arrows,
  onScroll,
  ...rest
}: RowProps & {
  children: ReactNode
  /** The row's own spacing: its gap, and the padding (with matching negative margins) that
   *  leaves room for shadows, hover lifts and focus rings. */
  className?: string
  style?: CSSProperties
  /** How far the edges fade where there is more to show, in pixels; none at 0. */
  fade?: number
  /** Arrow buttons on hover; `top` sets them over the covers of a row of cards (centred
   *  otherwise); `inside` keeps them within the row (a narrow pane with no room beside it). */
  arrows?: { top?: number; inside?: boolean }
  /** Called as the row scrolls (a row that draws its cards as they come near). */
  onScroll?: (el: HTMLDivElement) => void
}) {
  const row = useRef<HTMLDivElement | null>(null)
  const [ends, setEnds] = useState({ start: true, end: true })
  const measure = () => {
    const el = row.current
    if (!el) return
    const now = rowEnds({ left: el.scrollLeft, width: el.scrollWidth, client: el.clientWidth })
    setEnds((was) => (was.start === now.start && was.end === now.end ? was : now))
  }
  // Again whenever what it holds changes (chips arriving, a shelf filling in), and as it resizes.
  useLayoutEffect(measure)
  useLayoutEffect(() => {
    const el = row.current
    if (!el) return
    const ro = new ResizeObserver(measure)
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  // The row's own glide, for the wheel's clicks, the arrows and the keys.
  const glider = useRef<Glider | null>(null)
  useEffect(() => {
    const el = row.current
    if (!el) return
    const g = (glider.current = new Glider(el))
    // A click of a sideways wheel. Not a React handler, which can't keep the page from
    // scrolling too.
    const onWheel = (e: WheelEvent) => {
      const by = rowWheel(e, g.box())
      if (by === null) return
      e.preventDefault()
      g.by(by)
    }
    el.addEventListener('wheel', onWheel, { passive: false })
    return () => {
      el.removeEventListener('wheel', onWheel)
      g.stop()
    }
  }, [])

  const page = (dir: 1 | -1) => {
    const el = row.current
    if (el) glider.current?.by(dir * pageBy(el.clientWidth))
  }

  // Left and Right from one item to the next: its first control takes focus, and the row brings
  // it into view.
  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    rest.onKeyDown?.(e)
    if (e.defaultPrevented || e.altKey || e.ctrlKey || e.metaKey || e.shiftKey || editable(e.target)) return
    const items = [...e.currentTarget.children].filter((c): c is HTMLElement => c instanceof HTMLElement && !!first(c))
    const to = stepItem(items.findIndex((c) => c.contains(e.target as Node)), e.key, items.length)
    if (to === null) return
    e.preventDefault()
    first(items[to])?.focus({ preventScroll: true })
    const g = glider.current
    if (!g) return
    const left = revealLeft(g.box(), g.span(items[to]), Math.max(g.inset(), fade), g.stops())
    if (left !== null) g.to(left)
  }

  const scroller = (
    <div
      {...rest}
      ref={row}
      onScroll={(e) => {
        onScroll?.(e.currentTarget)
        measure()
      }}
      onKeyDown={onKeyDown}
      onPointerEnter={measure}
      data-fade={fade > 0 || undefined}
      data-more-start={!ends.start || undefined}
      data-more-end={!ends.end || undefined}
      className={`hrow flex ${className}`}
      style={fade > 0 ? ({ ...style, '--hrow-fade': `${fade}px` } as CSSProperties) : style}
    >
      {children}
    </div>
  )
  if (!arrows) return scroller

  const arrow = (dir: 1 | -1) => (
    <button
      aria-label={dir < 0 ? 'Scroll left' : 'Scroll right'}
      // Off the Tab order: Tab reaches every item, and Left and Right move along them.
      tabIndex={-1}
      onClick={() => page(dir)}
      style={arrows.top === undefined ? undefined : { top: arrows.top }}
      className={`popover absolute z-10 grid h-10 w-10 place-items-center rounded-full text-ink/80 opacity-0 transition-opacity duration-200 hover:text-ink group-hover/row:opacity-100 focus-visible:opacity-100 ${
        arrows.top === undefined ? 'top-1/2 -translate-y-1/2' : ''
      } ${dir < 0 ? (arrows.inside ? 'left-1' : '-left-4') : arrows.inside ? 'right-1' : '-right-4'}`}
    >
      <IconChevron className={dir < 0 ? 'rotate-180' : ''} />
    </button>
  )
  return (
    <div className="group/row relative">
      {!ends.start && arrow(-1)}
      {scroller}
      {!ends.end && arrow(1)}
    </div>
  )
}

const FOCUSABLE = 'a[href], button:not([disabled]), input, select, textarea, [tabindex]:not([tabindex="-1"])'
const first = (el: HTMLElement) => (el.matches(FOCUSABLE) ? el : el.querySelector<HTMLElement>(FOCUSABLE))
const editable = (t: EventTarget) =>
  t instanceof HTMLElement && (t.isContentEditable || t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT')

/**
 * Glides a row sideways, frame by frame, so Venuta's Motion setting decides how it moves (the
 * browser's own smooth scrolling follows Windows' animation setting alone). With less motion it
 * moves at once. The row's snapping rests while it glides (it would pull every frame back to a
 * card) and every glide ends on a card in a row that snaps. A touchpad or a drag in the middle
 * of a glide takes over from it.
 */
class Glider {
  private el: HTMLElement
  private heading: number | null = null
  private frame = 0
  private snapping: boolean | null = null
  private last = 0

  constructor(el: HTMLElement) {
    this.el = el
  }

  box() {
    return { left: this.el.scrollLeft, width: this.el.scrollWidth, client: this.el.clientWidth }
  }

  /** The row's scroll padding: where its snapping lines a card up. */
  inset() {
    return parseFloat(getComputedStyle(this.el).scrollPaddingInlineStart) || 0
  }

  /** An item's start and end within the row's content. */
  span(c: HTMLElement) {
    const x0 = this.el.getBoundingClientRect().left - this.el.scrollLeft
    const r = c.getBoundingClientRect()
    return { start: r.left - x0, end: r.right - x0 }
  }

  /** Where a row that snaps can stop (each item's start), none for one that doesn't. */
  stops(): number[] {
    // Read while it rests: a glide turns the snapping off for its length.
    this.snapping ??= !getComputedStyle(this.el).scrollSnapType.startsWith('none')
    if (!this.snapping) return []
    const inset = this.inset()
    return [...this.el.children].filter((c): c is HTMLElement => c instanceof HTMLElement).map((c) => this.span(c).start - inset)
  }

  /** On by `by` from where it is heading. */
  by(by: number) {
    this.to(scrollTarget(this.box(), this.heading ?? this.el.scrollLeft, by, this.stops()))
  }

  to(left: number) {
    const el = this.el
    cancelAnimationFrame(this.frame)
    if (document.documentElement.dataset.motion === 'reduced') {
      this.end()
      el.scrollLeft = left
      return
    }
    const from = el.scrollLeft
    if (Math.abs(left - from) < 1) return this.end()
    this.heading = left
    el.style.scrollSnapType = 'none'
    this.last = from
    const t0 = performance.now()
    const step = (now: number) => {
      // Something else moved it (a touchpad, a drag): that wins.
      if (Math.abs(el.scrollLeft - this.last) > 2) return this.end()
      const x = glideAt(from, left, now - t0)
      el.scrollLeft = x
      this.last = el.scrollLeft
      if (now - t0 < GLIDE_MS) this.frame = requestAnimationFrame(step)
      else this.end()
    }
    this.frame = requestAnimationFrame(step)
  }

  private end() {
    cancelAnimationFrame(this.frame)
    this.heading = null
    this.el.style.scrollSnapType = ''
    this.snapping = null
  }

  stop() {
    this.end()
  }
}
