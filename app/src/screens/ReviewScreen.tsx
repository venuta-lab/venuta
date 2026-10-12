import { AnimatePresence, motion } from 'motion/react'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import {
  answerQuestion,
  applyApproved,
  cancelApply,
  FIELD_NAMES,
  inApp,
  listRuns,
  review,
  setDecisions,
  setSettings,
  undoRun,
  type AlbumView,
  type ApplyReport,
  type Decision,
  type Review,
  type Run,
} from '../lib/engine'
import { setAccent, useCovers } from '../lib/covers'
import { IconChevron, IconFolder } from '../components/Icons'
import { PillButton } from './LibraryScreen'

const spring = { type: 'spring', stiffness: 380, damping: 34 } as const

type Tab = 'needs' | 'glance' | 'ready' | 'aside'
const TABS: { id: Tab; label: string }[] = [
  { id: 'needs', label: 'Needs you' },
  { id: 'glance', label: 'Worth a glance' },
  { id: 'ready', label: 'Ready' },
  { id: 'aside', label: 'Set aside' },
]

const tabOf = (a: AlbumView): Tab =>
  a.action.kind === 'quarantine'
    ? 'aside'
    : a.confidence === 'low'
      ? 'needs'
      : a.confidence === 'medium'
        ? 'glance'
        : 'ready'

export function ReviewScreen({ root }: { root: string }) {
  const [data, setData] = useState<Review | null>(null)
  const [error, setError] = useState('')
  const [tab, setTab] = useState<Tab>('needs')
  const [open_, setOpen] = useState<string | null>(null)
  const [building, setBuilding] = useState<{ done: number; total: number } | null>(null)
  const [result, setResult] = useState<ApplyReport | null>(null)
  const [runs, setRuns] = useState<Run[]>([])

  const load = useCallback(async () => {
    try {
      const [r, rs] = await Promise.all([review(root), listRuns()])
      setData(r)
      setRuns(rs)
      setError('')
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }, [root])

  useEffect(() => {
    if (root) void load()
  }, [root, load])

  const groups = useMemo(() => {
    const g: Record<Tab, AlbumView[]> = { needs: [], glance: [], ready: [], aside: [] }
    for (const a of data?.plan.albums ?? []) g[tabOf(a)].push(a)
    return g
  }, [data])

  if (!root) return <Centered text="Choose a library on the Library screen first." />
  if (error && !data) return <Centered text={error} />
  if (!data) return <Centered text="Working out what each album should look like…" />

  const decisions = data.decisions
  const approved = Object.values(decisions).filter((d) => d === 'approved').length
  const shown = groups[tab]

  async function decide(changes: Record<string, Decision | null>) {
    const d = await setDecisions(root, changes)
    setData((cur) => (cur ? { ...cur, decisions: d } : cur))
  }

  async function chooseTarget() {
    if (!inApp() || !data) return
    const picked = await open({ directory: true, title: 'Where should the new library be built?' })
    if (typeof picked !== 'string') return
    const settings = { ...data.settings, new_library: picked }
    await setSettings(settings)
    setData({ ...data, settings })
  }

  async function build() {
    setResult(null)
    setBuilding({ done: 0, total: 0 })
    try {
      const report = await applyApproved(root, (done, total) => setBuilding({ done, total }))
      setResult(report)
      setRuns(await listRuns())
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBuilding(null)
    }
  }

  async function answer(a: string, b: string, keep: string | null) {
    try {
      setData(await answerQuestion(root, a, b, keep))
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const c = (t: Tab) => groups[t].length
  return (
    <div className="mx-auto max-w-[980px] pb-20">
      <header className="pt-6">
        <h1 className="text-[44px] font-semibold leading-none tracking-[-0.035em]">Review</h1>
        <p className="mt-4 max-w-[64ch] text-[15px] leading-relaxed text-ink/70">
          {data.plan.albums.length} albums planned. {c('ready')} are ready to go, {c('glance')} are worth a
          glance, {c('needs')} need you, and {c('aside')} duplicates will be set aside. Your current library is
          never changed: approved albums are copied into the new one.
        </p>
      </header>

      <div className="glass mt-6 flex flex-wrap items-center gap-x-4 gap-y-3 rounded-2xl px-5 py-4">
        <div className="min-w-[200px] flex-1">
          <div className="text-[12.5px] text-ink/55">New library</div>
          <div className="truncate text-[14.5px]">{data.settings.new_library ?? 'Not chosen yet'}</div>
        </div>
        <PillButton onClick={chooseTarget} title="Choose where the new library is built">
          <IconFolder />
        </PillButton>
        <PillButton onClick={build} disabled={!approved || !!building || !data.settings.new_library} accent>
          {approved ? `Build ${approved} ${approved === 1 ? 'album' : 'albums'}` : 'Approve albums to build'}
        </PillButton>
      </div>

      <AnimatePresence>
        {building && (
          <motion.div
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: 'auto' }}
            exit={{ opacity: 0, height: 0 }}
            className="overflow-hidden"
          >
            <div className="glass mt-3 flex items-center gap-4 rounded-2xl px-5 py-3.5">
              <div className="flex-1">
                <div className="text-[13.5px]">
                  {building.total
                    ? `Building: ${building.done.toLocaleString()} of ${building.total.toLocaleString()} tracks`
                    : 'Starting…'}
                </div>
                <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-white/10">
                  <motion.div
                    className="h-full rounded-full"
                    style={{ background: 'var(--accent)' }}
                    animate={{ width: `${building.total ? (building.done / building.total) * 100 : 0}%` }}
                  />
                </div>
              </div>
              <button
                onClick={() => void cancelApply()}
                className="rounded-full px-3 py-1.5 text-[13px] text-ink/70 hover:bg-white/10"
              >
                Stop
              </button>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      {result && <BuildResult report={result} onDismiss={() => setResult(null)} />}
      {error && <p className="mt-3 text-[13px] text-problem">{error}</p>}

      {data.plan.questions.length > 0 && (
        <section className="mt-8">
          <h2 className="mb-3 text-[15px] font-medium text-ink/80">Only you can answer these</h2>
          <div className="grid gap-2">
            {data.plan.questions.map((q) => (
              <motion.div layout key={q.id} className="glass rounded-2xl px-5 py-4">
                <div className="text-[14.5px]">{q.text}</div>
                <div className="mt-3 flex flex-wrap gap-2">
                  <ChoiceButton onClick={() => answer(q.a, q.b, q.a)}>Same, call them “{q.a}”</ChoiceButton>
                  <ChoiceButton onClick={() => answer(q.a, q.b, q.b)}>Same, call them “{q.b}”</ChoiceButton>
                  <ChoiceButton onClick={() => answer(q.a, q.b, null)}>Different people</ChoiceButton>
                </div>
              </motion.div>
            ))}
          </div>
        </section>
      )}

      <div className="mt-8 flex flex-wrap items-center gap-3">
        <div className="glass flex flex-wrap rounded-full p-1">
          {TABS.map((t) => (
            <button
              key={t.id}
              onClick={() => setTab(t.id)}
              className={`relative rounded-full px-4 py-1.5 text-[13px] ${tab === t.id ? 'text-ink' : 'text-ink/60 hover:text-ink/85'}`}
            >
              {tab === t.id && (
                <motion.span
                  layoutId="review-tab"
                  className="absolute inset-0 rounded-full"
                  style={{ background: 'color-mix(in oklab, var(--accent) 28%, rgb(255 255 255 / 0.06))' }}
                  transition={spring}
                />
              )}
              <span className="relative">
                {t.label} <span className="text-ink/45">{c(t.id)}</span>
              </span>
            </button>
          ))}
        </div>
        {tab !== 'aside' && shown.length > 0 && (
          <button
            onClick={() => decide(Object.fromEntries(shown.map((a) => [a.id, 'approved' as Decision])))}
            className="ml-auto rounded-full px-4 py-1.5 text-[13px] text-ink/75 hover:bg-white/10 hover:text-ink"
          >
            Approve all {shown.length}
          </button>
        )}
        {tab === 'aside' && shown.length > 0 && (
          <button
            onClick={() => decide(Object.fromEntries(shown.map((a) => [a.id, 'approved' as Decision])))}
            className="ml-auto rounded-full px-4 py-1.5 text-[13px] text-ink/75 hover:bg-white/10 hover:text-ink"
          >
            Set all {shown.length} aside
          </button>
        )}
      </div>

      <div className="mt-3 flex flex-col gap-2">
        {shown.length === 0 && <p className="py-10 text-center text-[14px] text-ink/50">Nothing here.</p>}
        {shown.map((a) => (
          <AlbumRow
            key={a.id}
            album={a}
            decision={decisions[a.id]}
            open={open_ === a.id}
            onToggle={() => setOpen(open_ === a.id ? null : a.id)}
            onDecide={(d) => decide({ [a.id]: d })}
          />
        ))}
      </div>

      {runs.length > 0 && (
        <section className="mt-12">
          <h2 className="mb-3 text-[15px] font-medium text-ink/80">Previous builds</h2>
          <div className="grid gap-2">
            {runs.map((r) => (
              <RunRow key={r.journal} run={r} onUndone={load} />
            ))}
          </div>
        </section>
      )}
    </div>
  )
}

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
  const cover = useCovers(props.coverSize ?? 320)(a.id)
  const approved = props.decision === 'approved'
  const aside = a.action.kind === 'quarantine'
  return (
    <motion.div layout transition={spring} className="glass overflow-hidden rounded-[18px]">
      <div className="flex items-center gap-4 px-4 py-3">
        <motion.div
          onHoverStart={() => setAccent(cover?.accent)}
          className="h-14 w-14 shrink-0 overflow-hidden rounded-xl bg-white/5"
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
            className="border-t border-white/[0.06] px-5 pb-5 pt-4"
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
      <div className="whitespace-pre-line break-words text-ink/50 line-through decoration-white/20">{list(ch.before)}</div>
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
        style={{ background: props.on ? 'var(--accent)' : 'rgb(255 255 255 / 0.12)' }}
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

function ChoiceButton(props: { onClick: () => void; children: React.ReactNode }) {
  return (
    <motion.button
      whileTap={{ scale: 0.96 }}
      onClick={props.onClick}
      className="rounded-full bg-white/[0.08] px-4 py-1.5 text-[13px] hover:bg-white/[0.14]"
    >
      {props.children}
    </motion.button>
  )
}

function BuildResult(props: { report: ApplyReport; onDismiss: () => void }) {
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

function RunRow(props: { run: Run; onUndone: () => void }) {
  const [state, setState] = useState<'idle' | 'confirm' | 'working'>('idle')
  const when = new Date(props.run.started_ms).toLocaleString()
  return (
    <div className="glass flex items-center gap-4 rounded-2xl px-5 py-3 text-[13.5px]">
      <div className="min-w-0 flex-1">
        <div>{when}</div>
        <div className="truncate text-[12px] text-ink/50">
          {props.run.files.toLocaleString()} files created in {props.run.target}
        </div>
      </div>
      {state === 'confirm' ? (
        <>
          <span className="text-[12.5px] text-ink/70">Remove everything this build created?</span>
          <button
            onClick={async () => {
              setState('working')
              await undoRun(props.run.journal)
              props.onUndone()
            }}
            className="rounded-full bg-[color-mix(in_oklab,var(--color-problem)_30%,transparent)] px-3 py-1.5"
          >
            Undo build
          </button>
          <button onClick={() => setState('idle')} className="rounded-full px-3 py-1.5 text-ink/60 hover:bg-white/10">
            Keep
          </button>
        </>
      ) : (
        <button
          disabled={state === 'working'}
          onClick={() => setState('confirm')}
          className="rounded-full px-3 py-1.5 text-ink/70 hover:bg-white/10"
        >
          {state === 'working' ? 'Undoing…' : 'Undo…'}
        </button>
      )}
    </div>
  )
}

function Centered({ text }: { text: string }) {
  return <div className="grid h-full place-items-center text-[14px] text-ink/60">{text}</div>
}
