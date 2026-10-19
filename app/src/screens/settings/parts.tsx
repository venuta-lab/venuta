import { AnimatePresence, motion } from 'motion/react'
import { useState, type ReactNode } from 'react'

/*
  The pieces every Settings page is made of: a row (what it is, a line on what it does, its
  control on the right), a group heading, and More (the rarely needed rows of a group, one click
  deeper, never gone).
*/

export function Row(props: { title: string; hint?: ReactNode; children?: ReactNode }) {
  return (
    <div className="flex flex-wrap items-center gap-4 border-b border-edge py-4 last:border-b-0">
      <div className="min-w-[220px] flex-1">
        <div className="text-[14px] font-medium">{props.title}</div>
        {props.hint && <div className="mt-0.5 max-w-[68ch] text-[12.5px] leading-relaxed text-ink/55">{props.hint}</div>}
      </div>
      {props.children}
    </div>
  )
}

/** A group of rows on a page, under its name. */
export function Group(props: { title: string; line?: ReactNode; children: ReactNode }) {
  return (
    <section className="mt-7 first:mt-5">
      <h3 className="text-[13px] font-semibold text-ink/60">{props.title}</h3>
      {props.line && <p className="mt-1 max-w-[68ch] text-[12.5px] leading-relaxed text-ink/55">{props.line}</p>}
      <div>{props.children}</div>
    </section>
  )
}

/** A page's title and the one line under it. */
export function PageHead(props: { title: string; line?: ReactNode }) {
  return (
    <div>
      <h2 className="title text-[22px]">{props.title}</h2>
      {props.line && <p className="mt-1 max-w-[66ch] text-[13px] leading-relaxed text-ink/55">{props.line}</p>}
    </div>
  )
}

/** Rows most people never need: closed until asked for, with what is inside named on the row. */
export function More(props: { title: string; hint: ReactNode; children: ReactNode }) {
  const [open, setOpen] = useState(false)
  return (
    <div className="border-b border-edge last:border-b-0">
      <button
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        className="flex w-full flex-wrap items-center gap-4 py-4 text-left"
      >
        <span className="min-w-[220px] flex-1">
          <span className="block text-[14px] font-medium">{props.title}</span>
          <span className="mt-0.5 block max-w-[68ch] text-[12.5px] leading-relaxed text-ink/55">{props.hint}</span>
        </span>
        <svg
          width="14"
          height="14"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          aria-hidden
          className={`text-ink/50 transition-transform duration-200 ${open ? 'rotate-90' : ''}`}
        >
          <path d="m9 6 6 6-6 6" />
        </svg>
      </button>
      <AnimatePresence initial={false}>
        {open && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: 'auto', opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.2 }}
            className="overflow-hidden"
          >
            <div className="mb-3 rounded-2xl bg-ink/[0.03] px-4">{props.children}</div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  )
}
