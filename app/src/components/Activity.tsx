import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useRef, useState } from 'react'
import { counted, fraction, timeLeft, useJobs, useNow, type Job } from '../lib/jobs'

/** A small ring: filled by progress, or a turning arc while the amount of work is unknown. */
export function Ring({ value, size = 14 }: { value: number | null; size?: number }) {
  const r = (size - 3) / 2
  const c = 2 * Math.PI * r
  return (
    <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} className={value === null ? 'spin' : ''} aria-hidden>
      <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="currentColor" strokeOpacity={0.18} strokeWidth={2} />
      <circle
        cx={size / 2}
        cy={size / 2}
        r={r}
        fill="none"
        stroke="var(--accent)"
        strokeWidth={2}
        strokeLinecap="round"
        strokeDasharray={c}
        strokeDashoffset={value === null ? c * 0.72 : c * (1 - value)}
        transform={`rotate(-90 ${size / 2} ${size / 2})`}
        style={{ transition: 'stroke-dashoffset 300ms ease' }}
      />
    </svg>
  )
}

/** A progress bar; without a known total it sweeps instead of filling. */
export function Bar({ value }: { value: number | null }) {
  return (
    <div className="relative h-1.5 overflow-hidden rounded-full bg-ink/10" role="progressbar" aria-valuenow={value === null ? undefined : Math.round(value * 100)}>
      {value === null ? (
        <div className="bar-sweep absolute inset-y-0 w-1/3 rounded-full bg-accent" />
      ) : (
        <div className="h-full rounded-full bg-accent transition-[width] duration-300" style={{ width: `${Math.max(2, value * 100)}%` }} />
      )}
    </div>
  )
}

/** One running job, as a row: what, where it is, how long, and a way to stop it. */
export function JobRow({ job, now }: { job: Job; now: number }) {
  const count = counted(job)
  const eta = timeLeft(job, now)
  return (
    <div className="py-3">
      <div className="flex items-baseline gap-3">
        <span className="min-w-0 flex-1 truncate text-[13.5px] font-medium text-ink">{job.label}</span>
        {job.cancel && (
          <button onClick={job.cancel} className="shrink-0 rounded-full px-2.5 py-0.5 text-[12px] text-ink/60 hover:bg-ink/10 hover:text-ink">
            Stop
          </button>
        )}
      </div>
      <div className="mt-2">
        <Bar value={fraction(job)} />
      </div>
      <div className="mt-1.5 flex gap-3 text-[12px] text-ink/55">
        <span className="min-w-0 flex-1 truncate">{job.note || count || 'Starting…'}</span>
        <span className="shrink-0">{job.note && count ? `${count}${eta ? `, ${eta}` : ''}` : eta}</span>
      </div>
    </div>
  )
}

/**
 * Title-bar indicator: hidden when nothing runs; otherwise the current work at a glance, and a
 * list of everything running on click.
 */
export function ActivityIndicator() {
  const jobs = useJobs()
  const [want, setOpen] = useState(false)
  const open = want && jobs.length > 0
  const box = useRef<HTMLDivElement>(null)
  const now = useNow(jobs.length > 0)
  const lead = jobs[jobs.length - 1]

  useEffect(() => {
    if (!open) return
    const close = (e: PointerEvent) => !box.current?.contains(e.target as Node) && setOpen(false)
    const esc = (e: KeyboardEvent) => e.key === 'Escape' && setOpen(false)
    window.addEventListener('pointerdown', close)
    window.addEventListener('keydown', esc)
    return () => {
      window.removeEventListener('pointerdown', close)
      window.removeEventListener('keydown', esc)
    }
  }, [open])

  return (
    <div ref={box} className="relative mr-2 flex items-center">
      <AnimatePresence>
        {lead && (
          <motion.button
            key="pill"
            initial={{ opacity: 0, scale: 0.9 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.9 }}
            onClick={() => setOpen((o) => !o)}
            aria-expanded={open}
            aria-label="Work in progress"
            className="flex max-w-[300px] items-center gap-2 rounded-full bg-ink/[0.07] py-1 pl-2.5 pr-3 text-[12.5px] text-ink/75 hover:bg-ink/[0.11]"
          >
            <Ring value={fraction(lead)} />
            <span className="truncate">{lead.label}</span>
            {fraction(lead) !== null && <span className="shrink-0 text-ink/50">{Math.floor((fraction(lead) ?? 0) * 100)}%</span>}
            {jobs.length > 1 && <span className="shrink-0 rounded-full bg-ink/10 px-1.5 text-[11px] text-ink/70">+{jobs.length - 1}</span>}
          </motion.button>
        )}
      </AnimatePresence>
      <AnimatePresence>
        {open && (
          <motion.div
            initial={{ opacity: 0, y: -6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }}
            transition={{ duration: 0.16 }}
            className="popover absolute right-0 top-9 z-50 w-[360px] rounded-2xl px-4 py-1"
            role="dialog"
            aria-label="Running now"
          >
            <div className="divide-y divide-ink/[0.07]">
              {[...jobs].reverse().map((j) => (
                <JobRow key={j.key} job={j} now={now} />
              ))}
            </div>
            <p className="border-t border-ink/[0.07] py-2.5 text-[11.5px] text-ink/45">You can keep using Venuta while this runs.</p>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  )
}
