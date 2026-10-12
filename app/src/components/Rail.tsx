import { motion } from 'motion/react'
import type { ComponentType, SVGProps } from 'react'

export interface Section {
  id: string
  label: string
  icon: ComponentType<SVGProps<SVGSVGElement>>
  /** Keyboard shortcut shown in the tooltip. */
  keys: string
  /** A count shown as a badge (things waiting), if any. */
  badge?: number
}

/** The left rail: your places on top, history and settings at the bottom. */
export function Rail(props: {
  top: Section[]
  bottom: Section[]
  active: string
  onSelect: (id: string) => void
}) {
  const item = (s: Section) => {
    const on = s.id === props.active
    const Icon = s.icon
    return (
      <button
        key={s.id}
        onClick={() => props.onSelect(s.id)}
        title={`${s.label}  (${s.keys})`}
        aria-current={on ? 'page' : undefined}
        className={`relative flex w-[68px] flex-col items-center gap-1 rounded-2xl py-2.5 text-[11px] transition-colors ${
          on ? 'text-ink' : 'text-ink/55 hover:text-ink/85'
        }`}
      >
        {on && (
          <motion.span
            layoutId="rail-pill"
            className="absolute inset-0 rounded-2xl"
            style={{
              background: 'color-mix(in oklab, var(--accent) 26%, color-mix(in oklab, var(--ink) 6%, transparent))',
              boxShadow: 'inset 0 1px 0 var(--highlight)',
            }}
            transition={{ type: 'spring', stiffness: 420, damping: 34 }}
          />
        )}
        <span className="relative">
          <Icon />
          {!!s.badge && (
            <span className="absolute -right-2.5 -top-1.5 min-w-[18px] rounded-full bg-accent px-1 text-center text-[10px] font-semibold leading-[17px] text-on-accent">
              {s.badge > 99 ? '99+' : s.badge}
            </span>
          )}
        </span>
        <span className="relative">{s.label}</span>
      </button>
    )
  }
  return (
    <nav
      aria-label="Sections"
      className="glass relative z-20 mb-3 ml-3 flex w-[84px] shrink-0 flex-col items-center gap-1 rounded-[var(--radius-glass)] py-3"
    >
      {props.top.map(item)}
      <div className="flex-1" />
      {props.bottom.map(item)}
    </nav>
  )
}
