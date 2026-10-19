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
import { Page } from '../components/ui'
import { when } from '../lib/format'
import { lyricsChanged } from '../lib/lyrics'

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
    <Page title="Activity" line="Everything that changed, newest first. Undo puts things back exactly; your originals are never touched by a build.">
      <div className="max-w-[880px]">
      {nothing && (
        <div className="glass grid place-items-center rounded-[24px] px-6 py-14 text-center">
          <span className="grid h-14 w-14 place-items-center rounded-2xl bg-ink/[0.07] text-ink/60" aria-hidden>
            <svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
              <circle cx="12" cy="12" r="8.5" />
              <path d="M12 7.5V12l3 2" />
            </svg>
          </span>
          <p className="mt-4 text-[15px] font-medium">Nothing has changed yet</p>
          <p className="mt-1 max-w-[52ch] text-[13px] leading-relaxed text-ink/55">
            Builds, edits to tags, and your answers in Organize are listed here as they happen, each one with Undo.
          </p>
        </div>
      )}

      {(runs?.length ?? 0) > 0 && (
        <section>
          <h2 className="mb-3 text-[17px] font-semibold tracking-[-0.01em]">Builds and tidies</h2>
          <div className="grid gap-2">{runs?.map((r) => <RunRow key={r.journal} run={r} onUndone={load} />)}</div>
        </section>
      )}

      {edits.length > 0 && (
        <section className="mt-10">
          <h2 className="mb-3 text-[17px] font-semibold tracking-[-0.01em]">Edits to your files</h2>
          <div className="grid gap-2">
            {edits.map((e) => (
              <EditRow key={e.journal} change={e} onUndone={load} />
            ))}
          </div>
        </section>
      )}

      {answerList.length > 0 && (
        <section className="mt-10">
          <h2 className="mb-3 text-[17px] font-semibold tracking-[-0.01em]">Your answers in Organize</h2>
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
                    className="btn-ghost btn-sm"
                  >
                    Undo
                  </button>
                </div>
              )
            })}
          </div>
          <p className="mt-2 px-4 text-[12px] text-ink/45">Answers about artist names are kept in Organize, How artists are written.</p>
        </section>
      )}
      </div>
    </Page>
  )
}

function EditRow(props: { change: EditChange; onUndone: () => void }) {
  const [state, setState] = useState<'idle' | 'working'>('idle')
  const c = props.change
  return (
    <div className="glass flex items-center gap-4 rounded-2xl px-5 py-3 text-[13.5px]">
      <div className="min-w-0 flex-1">
        <div className="font-medium">
          {c.kind === 'lyrics' ? 'Lyrics timed' : `${c.files.toLocaleString('en-IN')} ${c.files === 1 ? 'file' : 'files'} edited`}
          <span className="font-normal text-ink/50">, {when(c.started_ms)}</span>
        </div>
        <div className="truncate text-[12px] text-ink/50">
          {c.examples.map((x) => (c.kind === 'lyrics' ? x.split('/').pop()?.replace(/\.lrc$/i, '') : x.split('/').pop())).join(', ')}
        </div>
      </div>
      <button
        disabled={state === 'working'}
        onClick={async () => {
          setState('working')
          await undoEdit(c.journal)
          if (c.kind === 'lyrics') lyricsChanged()
          props.onUndone()
        }}
        className="btn btn-sm"
      >
        {state === 'working' ? 'Undoing…' : 'Undo'}
      </button>
    </div>
  )
}
