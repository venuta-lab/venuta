import { motion } from 'motion/react'
import { createContext, use, useEffect, useId, useRef, useState, type ReactNode } from 'react'
import { createPortal } from 'react-dom'

/*
  The few controls every screen shares, so they look and move the same everywhere:
  Segmented (tabs and either-or choices), Switch (on or off), Chip (a filter) and Page
  (the frame of a place: its title, a bar that stays pinned while you scroll, the content).
*/

const spring = { type: 'spring', stiffness: 420, damping: 36 } as const

/** Pick one of a few: a row of pills with one sliding highlight. Used for tabs too. */
export function Segmented<T extends string>(props: {
  value: T
  options: { value: T; label: ReactNode; disabled?: boolean }[]
  onChange: (v: T) => void
  label: string
  /** Tabs switch views; otherwise it is a choice (radio). */
  tabs?: boolean
  size?: 'sm' | 'md'
  className?: string
}) {
  const id = useId()
  const sm = props.size === 'sm'
  return (
    <div
      role={props.tabs ? 'tablist' : 'radiogroup'}
      aria-label={props.label}
      className={`seg inline-flex shrink-0 rounded-full bg-ink/[0.07] p-1 shadow-[inset_0_0_0_1px_var(--edge)] ${props.className ?? ''}`}
    >
      {props.options.map((o) => {
        const on = o.value === props.value
        return (
          <button
            key={o.value}
            role={props.tabs ? 'tab' : 'radio'}
            aria-selected={props.tabs ? on : undefined}
            aria-checked={props.tabs ? undefined : on}
            disabled={o.disabled}
            onClick={() => props.onChange(o.value)}
            className={`seg-item relative rounded-full font-medium transition-colors disabled:opacity-35 ${
              sm ? 'px-3 py-1 text-[12px]' : 'px-4 py-1.5 text-[13px]'
            } ${on ? 'text-ink' : 'text-ink/60 hover:text-ink'}`}
          >
            {on && (
              <motion.span
                layoutId={`seg-${id}`}
                transition={spring}
                className="seg-on absolute inset-0 rounded-full bg-ink/[0.13] shadow-[inset_0_1px_0_var(--highlight),0_1px_6px_-2px_rgb(0_0_0/0.3)]"
              />
            )}
            <span className="relative">{o.label}</span>
          </button>
        )
      })}
    </div>
  )
}

/** On or off. */
export function Switch(props: { on: boolean; onChange: (on: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <button
      role="switch"
      aria-checked={props.on}
      aria-label={props.label}
      disabled={props.disabled}
      onClick={() => props.onChange(!props.on)}
      className={`relative h-6 w-11 shrink-0 rounded-full transition-colors duration-200 disabled:opacity-40 ${
        props.on ? 'bg-accent' : 'bg-ink/[0.16] shadow-[inset_0_0_0_1px_var(--edge)]'
      }`}
    >
      <motion.span
        animate={{ x: props.on ? 20 : 0 }}
        transition={spring}
        className="absolute left-0.5 top-0.5 h-5 w-5 rounded-full bg-white shadow-[0_1px_3px_rgb(0_0_0/0.35)]"
      />
    </button>
  )
}

/** A filter in a row of filters, with a sliding accent pill. */
export function Chip(props: { on: boolean; label: string; count?: number; group: string; onClick: () => void }) {
  return (
    <button
      onClick={props.onClick}
      aria-pressed={props.on}
      className={`relative shrink-0 rounded-full px-3.5 py-1.5 text-[12.5px] font-medium transition-colors ${props.on ? 'text-ink' : 'text-ink/60 hover:text-ink'}`}
    >
      {props.on && (
        <motion.span
          layoutId={props.group}
          transition={spring}
          className="absolute inset-0 rounded-full"
          style={{ background: 'color-mix(in oklab, var(--accent) 26%, color-mix(in oklab, var(--ink) 6%, transparent))' }}
        />
      )}
      <span className="relative">
        {props.label}
        {props.count !== undefined && <span className="font-normal text-ink/45"> {props.count.toLocaleString('en-IN')}</span>}
      </span>
    </button>
  )
}

/** The nearest scrolling ancestor (the page area). */
function scroller(el: HTMLElement | null): HTMLElement | null {
  for (let p = el?.parentElement ?? null; p; p = p.parentElement) {
    const o = getComputedStyle(p).overflowY
    if (o === 'auto' || o === 'scroll') return p
  }
  return null
}

/** True once `ref` has scrolled out of the top of its scroll area. */
function useScrolledPast(ref: React.RefObject<HTMLElement | null>) {
  const [past, setPast] = useState(false)
  useEffect(() => {
    const el = ref.current
    if (!el) return
    const root = scroller(el)
    const io = new IntersectionObserver(([e]) => setPast(!e.isIntersecting && e.boundingClientRect.top < (e.rootBounds?.top ?? 0) + 1), {
      root,
      threshold: 0,
    })
    io.observe(el)
    return () => io.disconnect()
  }, [ref])
  return past
}

/** Where content can put something into its page's pinned bar (a list's column heads). */
const BarSlot = createContext<HTMLElement | null>(null)

/** Show `children` in the page's pinned bar, under its tools, aligned with the content. */
export function InBar({ children }: { children: ReactNode }) {
  const slot = use(BarSlot)
  return slot ? createPortal(children, slot) : <>{children}</>
}

/** Every place's width: content lines up on one left edge across the whole app. */
const PAGE = 'mx-auto w-full max-w-[1440px]'

/**
 * The frame of a place. Its title and line, then a bar that pins to the top as you scroll
 * (tabs, search, filters), turning into a blurred strip with the title in small, so you
 * always know where you are and what you can do. Content scrolls beneath it.
 *
 * Lives directly in the page scroll area (which pads 40px each side), so the bar can reach
 * edge to edge.
 */
export function Page(props: {
  title: string
  line?: ReactNode
  /** Beside the title (the place's main actions). */
  actions?: ReactNode
  /** Pinned while scrolling: tabs, search, filters. */
  bar?: ReactNode
  /** A second row in the bar (filters, a list's column heads). */
  under?: ReactNode
  children: ReactNode
  className?: string
}) {
  const mark = useRef<HTMLDivElement>(null)
  const stuck = useScrolledPast(mark)
  const [slot, setSlot] = useState<HTMLElement | null>(null)
  const hasBar = !!props.bar || !!props.under
  return (
    <div className={`pb-16 ${props.className ?? ''}`}>
      <header className={`${PAGE} flex flex-wrap items-end gap-x-6 gap-y-3 pb-5 pt-7`}>
        <div className="min-w-0 flex-1">
          <h1 className="title text-[40px] leading-[1.05]">{props.title}</h1>
          {props.line && <div className="mt-2 max-w-[70ch] text-[14px] leading-relaxed text-ink/60">{props.line}</div>}
        </div>
        {props.actions && <div className="flex shrink-0 flex-wrap items-center gap-2">{props.actions}</div>}
      </header>
      <div ref={mark} aria-hidden className="h-px" />
      {hasBar ? (
        <div className={`sticky top-0 z-20 -mx-10 px-10 transition-[background-color,box-shadow] duration-200 ${stuck ? 'bar' : ''}`}>
          <div className={`${PAGE} py-2.5`}>
            <div className="flex min-h-[40px] flex-wrap items-center gap-3">
              <motion.span
                initial={false}
                animate={{ opacity: stuck ? 1 : 0, width: stuck ? 'auto' : 0, marginRight: stuck ? 4 : -12 }}
                transition={{ duration: 0.18 }}
                className="overflow-hidden whitespace-nowrap text-[15px] font-semibold tracking-[-0.01em]"
                aria-hidden={!stuck}
              >
                {props.title}
              </motion.span>
              {props.bar}
            </div>
            {props.under && <div className="mt-2">{props.under}</div>}
            <div ref={setSlot} className="empty:hidden [&>*]:mt-2" />
          </div>
        </div>
      ) : (
        // No tools to pin: just the title, in small, once the big one has scrolled away.
        <div className="pointer-events-none sticky top-0 z-20 -mx-10 h-0">
          <motion.div
            initial={false}
            animate={{ opacity: stuck ? 1 : 0, y: stuck ? 0 : -8 }}
            transition={{ duration: 0.18 }}
            className={`px-10 ${stuck ? 'bar pointer-events-auto' : ''}`}
          >
            <div className={`${PAGE} flex h-[52px] items-center text-[15px] font-semibold tracking-[-0.01em]`}>{props.title}</div>
          </motion.div>
        </div>
      )}
      <BarSlot value={hasBar ? slot : null}>
        <div className={`${PAGE} ${hasBar ? 'pt-3' : ''}`}>{props.children}</div>
      </BarSlot>
    </div>
  )
}

/**
 * A page opened from a place (an album, an artist): a pinned bar with Back, where the title
 * appears in small once the big one has scrolled away; `glow` (a cover) softly lights the top.
 */
export function Detail(props: {
  title: string
  onBack: () => void
  /** Beside the small title, once pinned (a play button). */
  tools?: ReactNode
  glow?: string
  children: ReactNode
}) {
  const top = useRef<HTMLDivElement>(null)
  const heading = useRef<HTMLDivElement>(null)
  // The bar gets its surface as soon as anything slides under it; the small title once the
  // big one has gone.
  const stuck = useScrolledPast(top)
  const titled = useScrolledPast(heading)
  return (
    <div className="relative -mx-10 px-10 pb-16">
      <div ref={top} aria-hidden className="absolute top-0 h-px w-px" />
      <div ref={heading} aria-hidden className="absolute top-[300px] h-px w-px" />
      {props.glow && (
        <div
          aria-hidden
          className="hero-glow pointer-events-none absolute inset-x-0 top-0 h-[520px] overflow-hidden opacity-45"
          style={{
            maskImage: 'radial-gradient(120% 90% at 30% 0%, #000 30%, transparent 75%)',
          }}
        >
          <img src={props.glow} alt="" className="hero-glow-img h-full w-full scale-125 object-cover" style={{ filter: "blur(64px) saturate(1.5) var(--art-tint)" }} />
        </div>
      )}
      <div className={`sticky top-0 z-20 -mx-10 px-10 transition-[background-color,box-shadow] duration-200 ${stuck ? 'bar' : ''}`}>
        <div className={`${PAGE} flex h-[52px] items-center gap-3`}>
          <button onClick={props.onBack} className="btn-ghost btn-sm -ml-3 gap-1.5" title="Back (Esc)">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
              <path d="m15 18-6-6 6-6" />
            </svg>
            Back
          </button>
          <motion.div
            initial={false}
            animate={{ opacity: titled ? 1 : 0, y: titled ? 0 : 6 }}
            transition={{ duration: 0.18 }}
            className="flex min-w-0 flex-1 items-center gap-3"
            aria-hidden={!titled}
          >
            <span className="title truncate text-[15px]">{props.title}</span>
            {titled && props.tools}
          </motion.div>
        </div>
      </div>
      <div className={`relative ${PAGE} pt-3`}>{props.children}</div>
    </div>
  )
}
