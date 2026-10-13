import { useCallback, useEffect, useState } from 'react'
import {
  clearAnswerQuick,
  listAnswers,
  listEdits,
  listRuns,
  undoEdit,
  type EditChange,
  type Run,
} from '../lib/engine'
import { RunRow } from '../components/AlbumRow'

/** What an answer was about, in words ("year:01_Hindi/…/2006_Gangster" → "Gangster: year"). */
function describeAnswer(id: string, choice: string): { what: string; answer: string } {
  const [kind, rest = ''] = id.split(/:(.*)/s)
  const album = (s: string) => {
    const dir = s.split('#')[0]
    return dir.split('/').pop()?.replace(/^\d{4}_/, '').replace(/_/g, ' ') ?? s
  }
  const answer =
    choice === 'approved'
      ? 'Approved'
      : choice.startsWith('y:')
        ? `Year ${choice.slice(2)}`
        : choice.startsWith('spell:')
          ? `Written “${choice.slice(6)}”`
          : choice === 'keep_both'
            ? 'Kept both copies'
            : choice === 'set_aside'
              ? 'Copies set aside'
              : choice === 'keep'
                ? 'Kept as singles'
                : choice === 'leave_out'
                  ? 'Left out'
                  : choice.startsWith('keep:')
                    ? `Kept the copy in ${album(choice.slice(5))}`
                    : choice
  switch (kind) {
    case 'year':
      return { what: `${album(rest)}: original year`, answer }
    case 'dup':
      return { what: `${album(rest)}: same songs`, answer }
    case 'spell':
      return { what: 'Spelling of a name', answer }
    case 'album':
      return { what: `${rest.split('/').pop()}: same album twice`, answer }
    case 'untagged':
      return { what: 'Files without tags', answer }
    default:
      return { what: id, answer }
  }
}

/** Everything that changed, newest first, each with undo. */
export function ActivityScreen(props: { root: string }) {
  const [runs, setRuns] = useState<Run[] | null>(null)
  const [edits, setEdits] = useState<EditChange[]>([])
  const [answers, setAnswers] = useState<Record<string, string>>({})
  const load = useCallback(() => {
    listRuns().then(setRuns).catch(() => setRuns([]))
    listEdits().then(setEdits).catch(() => setEdits([]))
    if (props.root) listAnswers(props.root).then(setAnswers).catch(() => setAnswers({}))
  }, [props.root])
  useEffect(() => {
    load()
  }, [load])

  const answerList = Object.entries(answers)
  const nothing = runs?.length === 0 && edits.length === 0 && answerList.length === 0
  return (
    <div className="mx-auto max-w-[820px] pb-16 pt-6">
      <h1 className="text-[40px] font-semibold leading-none tracking-[-0.035em]">Activity</h1>
      <p className="mt-2.5 max-w-[60ch] text-[14px] text-ink/60">
        Everything that changed, newest first. Undo puts things back exactly; your originals are never touched by a build.
      </p>
      {nothing && <p className="py-16 text-center text-[14px] text-ink/50">Nothing yet. Builds, edits and your answers will appear here.</p>}

      {(runs?.length ?? 0) > 0 && (
        <section className="mt-8">
          <h2 className="mb-3 text-[15px] font-medium text-ink/80">Builds</h2>
          <div className="grid gap-2">{runs?.map((r) => <RunRow key={r.journal} run={r} onUndone={load} />)}</div>
        </section>
      )}

      {edits.length > 0 && (
        <section className="mt-8">
          <h2 className="mb-3 text-[15px] font-medium text-ink/80">Edits to your files</h2>
          <div className="grid gap-2">
            {edits.map((e) => (
              <EditRow key={e.journal} change={e} onUndone={load} />
            ))}
          </div>
        </section>
      )}

      {answerList.length > 0 && (
        <section className="mt-8">
          <h2 className="mb-3 text-[15px] font-medium text-ink/80">Your answers in Organize</h2>
          <div className="grid gap-1">
            {answerList.map(([id, choice]) => {
              const d = describeAnswer(id, choice)
              return (
                <div key={id} className="flex items-center gap-3 rounded-xl px-4 py-2.5 text-[13.5px] hover:bg-ink/[0.04]">
                  <span className="min-w-0 flex-1 truncate">{d.what}</span>
                  <span className="shrink-0 text-ink/55">{d.answer}</span>
                  <button
                    onClick={async () => {
                      await clearAnswerQuick(props.root, id)
                      load()
                    }}
                    className="shrink-0 rounded-full px-3 py-1 text-[12.5px] text-ink/60 hover:bg-ink/10"
                  >
                    Undo
                  </button>
                </div>
              )
            })}
          </div>
          <p className="mt-2 px-4 text-[12px] text-ink/45">Answers about artist names are kept in Settings, Artist names.</p>
        </section>
      )}
    </div>
  )
}

function EditRow(props: { change: EditChange; onUndone: () => void }) {
  const [state, setState] = useState<'idle' | 'working'>('idle')
  const c = props.change
  return (
    <div className="glass flex items-center gap-4 rounded-2xl px-5 py-3 text-[13.5px]">
      <div className="min-w-0 flex-1">
        <div>
          {new Date(c.started_ms).toLocaleString()} · {c.files.toLocaleString()} {c.files === 1 ? 'file' : 'files'} changed
        </div>
        <div className="truncate text-[12px] text-ink/50">{c.examples.map((x) => x.split('/').pop()).join(', ')}</div>
      </div>
      <button
        disabled={state === 'working'}
        onClick={async () => {
          setState('working')
          await undoEdit(c.journal)
          props.onUndone()
        }}
        className="rounded-full px-3 py-1.5 text-ink/70 hover:bg-ink/10"
      >
        {state === 'working' ? 'Undoing…' : 'Undo'}
      </button>
    </div>
  )
}
