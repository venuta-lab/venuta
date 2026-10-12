import { motion } from 'motion/react'
import type { ComponentType, SVGProps } from 'react'

export interface Section {
  id: string
  label: string
  icon: ComponentType<SVGProps<SVGSVGElement>>
  ready: boolean
}

export function Rail(props: { sections: Section[]; active: string; onSelect: (id: string) => void }) {
  return (
    <nav className="glass relative z-20 ml-3 mb-3 flex w-[84px] shrink-0 flex-col items-center gap-1 rounded-[var(--radius-glass)] py-3">
      {props.sections.map((s, i) => {
        const on = s.id === props.active
        const Icon = s.icon
        return (
          <button
            key={s.id}
            onClick={() => props.onSelect(s.id)}
            title={`${s.label}  (Ctrl+${i + 1})`}
            className={`relative flex w-[68px] flex-col items-center gap-1 rounded-2xl py-2.5 text-[11px] transition-colors ${
              on ? 'text-ink' : 'text-ink/55 hover:text-ink/85'
            }`}
          >
            {on && (
              <motion.span
                layoutId="rail-pill"
                className="absolute inset-0 rounded-2xl"
                style={{
                  background: 'color-mix(in oklab, var(--accent) 26%, rgb(255 255 255 / 0.06))',
                  boxShadow: 'inset 0 1px 0 rgb(255 255 255 / 0.12)',
                }}
                transition={{ type: 'spring', stiffness: 420, damping: 34 }}
              />
            )}
            <Icon className="relative" />
            <span className="relative">{s.label}</span>
            {!s.ready && <span className="relative -mt-0.5 text-[9px] text-ink/35">soon</span>}
          </button>
        )
      })}
    </nav>
  )
}
