import { useState } from 'react'
import { correctLine } from '../../lib/lyricCorrections'
import { attributionOf, sourceKind, sourceWords, type LyricLine } from '../../lib/lyricsModel'
import { useEscape } from '../../lib/layers'
import { rowsFor, type SecondKind } from '../../lib/secondLines'
import { downloadModel, languageName, statusWords } from '../../lib/translation'
import { ENGINES } from '../../lib/translationEngine'
import type { SecondLinesView } from '../../lib/useSecondLines'
import { SECOND_NAMES } from '../../lib/secondChoice'

/*
  The second lines in Now Playing: the two switches over the words (with where the rows came from,
  and what Translation needs when the lyrics carry none in your language), and under each line its
  rows, each correctable by hand.
*/

/** The switches, in the app's own material: only those the song offers. */
export function SecondSwitches(props: { seconds: SecondLinesView; className?: string }) {
  const v = props.seconds
  const kinds = (['roman', 'translation'] as const).filter((k) => v.offered[k])
  if (!kinds.length && !v.dictionary) return null
  const notes = [v.notes.roman, v.notes.translation, v.dictionary].filter(Boolean)
  return (
    <div className={`flex flex-wrap items-center gap-x-3 gap-y-1.5 ${props.className ?? ''}`}>
      {kinds.length > 0 && <div role="group" aria-label="Under each line" className="inline-flex items-center gap-1 rounded-full bg-ink/[0.07] p-1 shadow-[inset_0_0_0_1px_var(--edge)]">
        {kinds.map((k) => (
          <button
            key={k}
            aria-pressed={v.on[k]}
            onClick={() => v.toggle(k)}
            title={`${SECOND_NAMES[k].line} (${SECOND_NAMES[k].keys}). Kept for ${v.lang ? `${languageName(v.lang)} songs` : 'songs in this language'}.`}
            className={`rounded-full px-3 py-1 text-[12.5px] font-medium transition-colors ${v.on[k] ? 'bg-ink/[0.13] text-ink shadow-[inset_0_1px_0_var(--highlight)]' : 'text-ink/55 hover:text-ink'}`}
          >
            {SECOND_NAMES[k].name}
          </button>
        ))}
      </div>}
      {notes.length > 0 && <span className="text-[12px] text-ink/45">{notes.join('. ')}</span>}
      <TranslationNote seconds={v} />
    </div>
  )
}

/** Translation switched on, and the lyrics carry none in your language: what it takes, in a
 *  sentence, with the one button that does it (Download, Translate, Try again). */
function TranslationNote({ seconds: v }: { seconds: SecondLinesView }) {
  const t = v.translation
  const words = statusWords(t, v.lang, v.reader)
  if (!words) return null
  const line = (tone: string, button?: { label: string; run: () => void }) => (
    <span className={`flex flex-wrap items-center gap-2 text-[12px] ${tone}`}>
      {words}
      {button && (
        <button onClick={button.run} className="btn btn-sm">
          {button.label}
        </button>
      )}
    </span>
  )
  switch (t.kind) {
    case 'needs-model':
      return line('text-ink/60', { label: 'Download', run: () => void downloadModel(t.model.id).catch(() => {}) })
    case 'ask':
      return line('text-ink/55', { label: `Translate with ${ENGINES[t.engine].name}`, run: v.translate })
    case 'failed':
      return line('text-ink/60', { label: 'Try again', run: v.translate })
    case 'translating':
    case 'downloading':
      return line('text-ink/55')
    default:
      return line('text-ink/45')
  }
}

/** A line's rows in Now Playing, each with its source on hover and a way to correct it. Rows a
 *  line has nothing for are left out here (the list scrolls; nothing has to hold still). */
export function SecondRowsList(props: { seconds: SecondLinesView; line: LyricLine | undefined; active: boolean }) {
  const { seconds: v, line } = props
  if (!line) return null
  const rows = rowsFor(line, v.rows, v.reader).filter((r) => r.text)
  if (!rows.length) return null
  return (
    <div className="mt-0.5 grid gap-0.5">
      {rows.map((r) => (
        <SecondRowLine key={r.kind} kind={r.kind} text={r.text} lang={r.second?.lang} source={r.second ? sourceWords(r.second.source) : ''} corrected={!!r.second && sourceKind(r.second.source).kind === 'Corrected'} attribution={attributionOf(r.second?.source)} line={line} songKey={v.key} active={props.active} />
      ))}
    </div>
  )
}

function SecondRowLine(props: { kind: SecondKind; text: string; lang?: string; source: string; corrected: boolean; attribution: string | null; line: LyricLine; songKey: string; active: boolean }) {
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(props.text)
  useEscape(editing, () => setEditing(false))
  const roman = props.kind === 'roman'
  const size = roman ? 'text-[16px] font-medium' : 'text-[15px]'
  const tone = props.active ? (roman ? 'text-ink/70' : 'text-ink/55') : roman ? 'text-ink/38' : 'text-ink/32'
  const save = () => {
    setEditing(false)
    if (draft.trim() !== props.text.trim()) correctLine(props.songKey, props.line.text, props.kind, draft)
  }
  if (editing)
    return (
      <span className="flex items-center gap-2">
        <input
          autoFocus
          value={draft}
          lang={props.lang}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') save()
          }}
          onBlur={() => setEditing(false)}
          aria-label={`Correct the ${roman ? 'pronunciation' : 'translation'}`}
          className={`field h-8 min-w-0 flex-1 rounded-lg px-2 ${size}`}
        />
        <span className="shrink-0 text-[11.5px] text-ink/45">Enter keeps it; empty goes back to the original</span>
      </span>
    )
  return (
    <span data-second={props.kind} className={`group/row flex items-baseline gap-2 ${size} leading-snug ${tone} transition-colors duration-300`}>
      <span lang={props.lang} title={props.source} className="min-w-0">
        {props.text}
      </span>
      {props.corrected && <span className="shrink-0 text-[11px] text-ink/35">corrected</span>}
      {/* What its translator asks to be shown beside it, always (not only on hover). */}
      {props.attribution && (
        <span data-attribution className="shrink-0 text-[11px] text-ink/45">
          {props.attribution}
        </span>
      )}
      <button
        onClick={() => {
          setDraft(props.text)
          setEditing(true)
        }}
        aria-label={`Correct this ${roman ? 'pronunciation' : 'translation'}`}
        title={`Correct this ${roman ? 'pronunciation' : 'translation'} (kept for this song). ${props.source}.`}
        className="shrink-0 rounded-full px-1.5 text-[11.5px] text-ink/45 opacity-0 transition-opacity hover:bg-ink/[0.08] hover:text-ink focus-visible:opacity-100 group-hover/row:opacity-100"
      >
        Correct
      </button>
    </span>
  )
}
