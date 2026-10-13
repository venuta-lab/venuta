import { AnimatePresence, motion } from 'motion/react'
import { useState } from 'react'
import { FIELD_NAMES, tidyUndo, undoRun, type AlbumView, type ApplyReport, type Decision, type Run } from '../lib/engine'
import { setAccent, useCovers } from '../lib/covers'
import { IconChevron } from './Icons'
import { when } from '../lib/format'

const spring = { type: 'spring', stiffness: 380, damping: 34 } as const

export function AlbumRow(props: {
  album: AlbumView
  decision?: Decision
  open: boolean
  onToggle: () => void
  onDecide: (d: Decision | null) => void
  /** Thumbnail size; incoming albums use their own so their covers never mix with the library's. */
  coverSize?: number
  approveLabel?: string
}) {
  const { album: a, open } = props
  const cover = useCovers(props.coverSize ?? 320)(a.cover ?? a.id)
  const approved = props.decision === 'approved'
  const aside = a.action.kind === 'quarantine'
  return (
    <motion.div layout transition={spring} className="glass overflow-hidden rounded-[18px]">
      <div className="flex items-center gap-4 px-4 py-3">
        <motion.div
          onHoverStart={() => setAccent(cover?.accent)}
          className="h-14 w-14 shrink-0 overflow-hidden rounded-xl bg-ink/5"
        >
          {cover && <img src={cover.src} alt="" className="h-full w-full object-cover" draggable={false} />}
        </motion.div>
        <button onClick={props.onToggle} aria-expanded={open} className="min-w-0 flex-1 text-left">
          <div className="truncate text-[15px] font-medium">{a.title}</div>
          <div className="truncate text-[12.5px] text-ink/55">
            {aside ? `Duplicate of ${a.action.kind === 'quarantine' ? a.action.duplicate_of : ''}` : a.target_dir}
          </div>
        </button>
        <span className="hidden shrink-0 text-[12px] text-ink/50 sm:block">
          {a.changes.length} changes, {a.tracks} tracks
        </span>
        <Toggle
          on={approved}
          label={aside ? 'Set aside' : (props.approveLabel ?? 'Approve')}
          onChange={(on) => props.onDecide(on ? 'approved' : null)}
        />
        <motion.button
          onClick={props.onToggle}
          animate={{ rotate: open ? 90 : 0 }}
          transition={spring}
          className="text-ink/50"
          aria-label="Show details"
        >
          <IconChevron />
        </motion.button>
      </div>
      <AnimatePresence initial={false}>
        {open && (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.2 }}
            className="border-t border-ink/[0.06] px-5 pb-5 pt-4"
          >
            {a.notes.map((n) => (
              <p key={n} className="mb-2 rounded-xl bg-[color-mix(in_oklab,var(--color-fix)_14%,transparent)] px-3.5 py-2 text-[13px] text-ink/85">
                {n}
              </p>
            ))}
            {a.art.kind === 'reencode' && (
              <p className="mb-2 text-[13px] text-ink/65">
                Cover art: re-saved so every player can show it (standard JPEG, up to {a.art.max_px}px). The full-size
                image is kept as cover.jpg.
              </p>
            )}
            <div className="mt-2 grid grid-cols-[150px_1fr_1fr] gap-x-4 gap-y-2 text-[12.5px]">
              <div className="text-ink/45">Field</div>
              <div className="text-ink/45">Now</div>
              <div className="text-ink/45">After</div>
              {a.changes.map((ch) => (
                <FieldRow key={ch.field} change={ch} />
              ))}
            </div>
            <details className="mt-4 text-[12.5px] text-ink/60">
              <summary className="cursor-pointer select-none text-ink/70">Files in the new library</summary>
              <div className="mt-2 grid gap-0.5">
                {a.files.map((f) => (
                  <span key={f} className="truncate">
                    {f}
                  </span>
                ))}
              </div>
            </details>
          </motion.div>
        )}
      </AnimatePresence>
    </motion.div>
  )
}

function FieldRow({ change: ch }: { change: AlbumView['changes'][number] }) {
  const name = FIELD_NAMES[ch.field.toUpperCase()] ?? ch.field
  const list = (v: string[]) => (v.length ? v.slice(0, 3).join('\n') + (v.length > 3 ? `\n+${v.length - 3} more` : '') : '(none)')
  return (
    <>
      <div className="pt-0.5" title={ch.reason}>
        <div className="text-ink/85">{name}</div>
        {ch.confidence !== 'high' && <div className="text-[11px] text-fix">check this</div>}
      </div>
      <div className="whitespace-pre-line break-words text-ink/50 line-through decoration-ink/20">{list(ch.before)}</div>
      <div className="whitespace-pre-line break-words text-ink/90">
        {list(ch.after)}
        <div className="mt-0.5 text-[11.5px] text-ink/45 no-underline">{ch.reason}</div>
      </div>
    </>
  )
}

function Toggle(props: { on: boolean; label: string; onChange: (on: boolean) => void }) {
  return (
    <button
      role="switch"
      aria-checked={props.on}
      onClick={() => props.onChange(!props.on)}
      className="flex shrink-0 items-center gap-2 text-[12.5px] text-ink/70"
    >
      <span
        className="relative h-6 w-10 rounded-full transition-colors"
        style={{ background: props.on ? 'var(--accent)' : 'color-mix(in oklab, var(--ink) 12%, transparent)' }}
      >
        <motion.span
          className="absolute top-1 h-4 w-4 rounded-full bg-white shadow"
          animate={{ left: props.on ? 20 : 4 }}
          transition={spring}
        />
      </span>
      {props.label}
    </button>
  )
}

export function BuildResult(props: { report: ApplyReport; onDismiss: () => void }) {
  const r = props.report
  return (
    <motion.div initial={{ y: 10, opacity: 0 }} animate={{ y: 0, opacity: 1 }} className="glass mt-3 rounded-2xl px-5 py-4">
      <div className="text-[14.5px]">
        {r.cancelled ? 'Stopped. ' : 'Done. '}
        {r.tracks_written.toLocaleString()} tracks written, {r.tracks_skipped.toLocaleString()} already up to date
        {r.quarantined_albums ? `, ${r.quarantined_albums} duplicate albums set aside` : ''} in{' '}
        {(r.elapsed_ms / 1000).toFixed(0)} s.
      </div>
      {r.errors.length > 0 && (
        <div className="mt-2 text-[13px] text-problem">
          {r.errors.length} files could not be written; they were left out and nothing was overwritten.
          <ul className="mt-1 list-disc pl-5 text-ink/70">
            {r.errors.slice(0, 5).map((e) => (
              <li key={e.album + e.file}>
                {e.file}: {e.message}
              </li>
            ))}
          </ul>
        </div>
      )}
      <button onClick={props.onDismiss} className="mt-2 text-[12.5px] text-ink/55 hover:text-ink">
        Dismiss
      </button>
    </motion.div>
  )
}

export function RunRow(props: { run: Run; onUndone: () => void }) {
  const [state, setState] = useState<'idle' | 'confirm' | 'working'>('idle')
  const [open, setOpen] = useState(false)
  const failed = props.run.failed ?? []
  const tidy = props.run.kind === 'tidy'
  return (
    <div className="glass rounded-2xl px-5 py-3 text-[13.5px]">
    <div className="flex items-center gap-4">
      <div className="min-w-0 flex-1">
        <div className="font-medium">
          {tidy ? 'Tidied your library' : 'Built your library'}
          <span className="font-normal text-ink/50">, {when(props.run.started_ms)}</span>
        </div>
        <div className="truncate text-[12px] text-ink/50">
          {tidy
            ? `${props.run.files.toLocaleString()} changes in ${props.run.target} (renamed or updated in place)`
            : `${props.run.files.toLocaleString()} files created in ${props.run.target}`}
          {props.run.finished === false && ', stopped before the end'}
        </div>
        {failed.length > 0 && (
          <button onClick={() => setOpen((o) => !o)} className="mt-1 text-[12px] text-problem hover:underline">
            {failed.length === 1 ? '1 file could not be built' : `${failed.length} files could not be built`}
            {open ? '' : ', see why'}
          </button>
        )}
      </div>
      {state === 'confirm' ? (
        <>
          <span className="text-[12.5px] text-ink/70">
            {tidy ? 'Put back everything this tidy changed?' : 'Remove everything this build created?'}
          </span>
          <button
            onClick={async () => {
              setState('working')
              if (tidy) await tidyUndo(props.run.target, props.run.journal)
              else await undoRun(props.run.journal)
              props.onUndone()
            }}
            className="btn btn-sm bg-[color-mix(in_oklab,var(--color-problem)_26%,transparent)] hover:bg-[color-mix(in_oklab,var(--color-problem)_36%,transparent)]"
          >
            {tidy ? 'Undo tidy' : 'Undo build'}
          </button>
          <button onClick={() => setState('idle')} className="btn-ghost btn-sm">
            Keep
          </button>
        </>
      ) : (
        <button
          disabled={state === 'working'}
          onClick={() => setState('confirm')}
          className="btn btn-sm"
        >
          {state === 'working' ? 'Undoing…' : 'Undo'}
        </button>
      )}
    </div>
      {open && (
        <ul className="mt-3 space-y-1.5 border-t border-edge pt-3 text-[12px]">
          {failed.map((f) => (
            <li key={f.file}>
              <div className="truncate text-ink/80">{f.file}</div>
              <div className="text-ink/50">{f.message}</div>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

