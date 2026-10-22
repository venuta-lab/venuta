import { invoke } from '@tauri-apps/api/core'
import { motion } from 'motion/react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { undoEdit } from '../../lib/engine'
import { useEscape } from '../../lib/layers'
import { lyricsChanged, type Lyrics } from '../../lib/lyrics'
import { notify } from '../../lib/notify'
import { getPlayerState, positionNow, seek, setStopAfter, toggle, usePlayerValue } from '../../lib/player'

/** A line and when it begins (null: not tapped yet). A break is a line with no words. */
interface Row {
  text: string
  ms: number | null
  /** When it began before (a line from the lyrics being re-timed), so its words move with it. */
  from?: number | null
  /** Its words' times [start, end, word], from the lyrics being re-timed. */
  words?: [number, number, string][]
  /** Its translation, from the lyrics being re-timed. */
  translation?: string | null
}

/** How much a nudge moves a line (Shift: more). */
const NUDGE_MS = 100
const NUDGE_BIG_MS = 500
/** Playing from a line starts a little before it, so you hear it arrive. */
const LEAD_IN_MS = 1500

/** 1:02.3 */
function stamp(ms: number | null): string {
  if (ms == null) return '–:––.–'
  const tenths = Math.round(ms / 100)
  const m = Math.floor(tenths / 600)
  const s = Math.floor((tenths % 600) / 10)
  return `${m}:${String(s).padStart(2, '0')}.${tenths % 10}`
}

const rowsFrom = (lyrics: Lyrics | null): Row[] =>
  lyrics?.lines.length
    ? lyrics.lines.map(([ms, text], i) => ({ text, ms, from: ms, words: lyrics.words?.[i], translation: lyrics.translations?.[i] ?? null }))
    : (lyrics?.text ?? '')
        .split(/\r?\n/)
        .map((l) => l.trim())
        .filter(Boolean)
        .map((text) => ({ text, ms: null }))

/**
 * Timing lyrics by tapping along: play the song, press Space as each line begins. Saved as a
 * .lrc beside the song (the song's own file is never touched), undoable in Activity.
 */
export function SyncMaker(props: {
  root: string
  path: string
  lyrics: Lyrics | null
  /** Its song is the one playing (another may have been started meanwhile). */
  current: boolean
  /** The song's title, for "Back to …". */
  title: string
  /** Play its song again. */
  onBack: () => void
  /** Whether there are taps not saved yet (so leaving can ask first). */
  onDirty?: (dirty: boolean) => void
  onClose: () => void
}) {
  const fixing = !!props.lyrics?.lines.length
  const [rows, setRows] = useState<Row[]>(() => rowsFrom(props.lyrics))
  const [pasted, setPasted] = useState('')
  // The next line to tap, and the line nudges move.
  const [cursor, setCursor] = useState(() => (fixing ? rows.length : 0))
  const [sel, setSel] = useState(-1)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState('')
  const [leaving, setLeaving] = useState(false)
  const playing = usePlayerValue((s) => s.playing)
  const position = usePlayerValue((s) => s.position_ms)
  const box = useRef<HTMLOListElement>(null)

  const tapped = rows.filter((r) => r.ms != null).length
  const changed = useRef(false)
  const { onClose } = props
  useEscape(true, () => (changed.current && !leaving ? setLeaving(true) : onClose()))

  // The line being sung, by the times so far: the one to watch while nudging.
  const singing = useMemo(() => {
    let found = -1
    rows.forEach((r, i) => {
      if (r.ms != null && r.ms <= position) found = i
    })
    return found
  }, [rows, position])

  const latest = useRef({ rows, cursor, sel, singing, current: props.current })
  useEffect(() => {
    latest.current = { rows, cursor, sel, singing, current: props.current }
  })

  // While timing, the song stops at its end instead of going on to the next one (which would
  // take the taps somewhere else); as it was, afterwards.
  useEffect(() => {
    const before = getPlayerState().stop_after_current
    void setStopAfter(true)
    return () => void setStopAfter(before)
  }, [])

  const { onDirty } = props
  function edit(next: Row[]) {
    changed.current = true
    onDirty?.(true)
    setLeaving(false)
    setRows(next)
  }

  function tap() {
    const { rows, cursor, current } = latest.current
    if (!current) return
    if (!getPlayerState().playing) {
      void toggle()
      return
    }
    if (cursor >= rows.length) return
    // Never before the line above it: a tap can't put lines out of order.
    const floor = rows[cursor - 1]?.ms ?? 0
    const next = rows.slice()
    next[cursor] = { ...next[cursor], ms: Math.max(floor, Math.round(positionNow())) }
    edit(next)
    setSel(cursor)
    setCursor(cursor + 1)
  }

  /** A break: the words leave the screen here (a long instrumental), until the next line. */
  function addBreak() {
    const { rows, cursor, current } = latest.current
    if (!current) return
    if (!getPlayerState().playing || cursor === 0) return
    const floor = rows[cursor - 1]?.ms ?? 0
    const next = rows.slice()
    next.splice(cursor, 0, { text: '', ms: Math.max(floor, Math.round(positionNow())) })
    edit(next)
    setSel(cursor)
    setCursor(cursor + 1)
  }

  function stepBack() {
    const { rows, cursor, current } = latest.current
    if (!current) return
    if (cursor === 0) return
    const back = cursor - 1
    const was = rows[back].ms ?? 0
    const next = rows.slice()
    if (!next[back].text) next.splice(back, 1)
    else next[back] = { ...next[back], ms: null }
    edit(next)
    setCursor(back)
    setSel(back - 1)
    // A few seconds back to tap it again, but not before the line above (a tap is never earlier
    // than the line before it).
    void seek(Math.max(rows[back - 1]?.ms ?? 0, was - 3000))
  }

  function nudge(by: number) {
    const { rows, sel, singing } = latest.current
    const i = sel >= 0 ? sel : singing
    if (i < 0 || rows[i].ms == null) return
    const lo = rows[i - 1]?.ms ?? 0
    const hi = rows[i + 1]?.ms ?? Infinity
    const next = rows.slice()
    next[i] = { ...next[i], ms: Math.min(hi, Math.max(lo, (rows[i].ms ?? 0) + by)) }
    edit(next)
    setSel(i)
  }

  function playFrom(i: number) {
    const ms = latest.current.rows[i]?.ms
    setSel(i)
    if (ms != null && latest.current.current) void seek(Math.max(0, ms - LEAD_IN_MS))
  }

  // The keys, ahead of the player's own (Space would otherwise pause).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target
      if (t instanceof HTMLElement && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA')) return
      if (!latest.current.rows.length) return
      const { rows, sel, current } = latest.current
      const take = () => {
        e.preventDefault()
        e.stopImmediatePropagation()
      }
      // Keys that would close Now Playing or change the song (and lose the taps) do nothing.
      if ((!e.ctrlKey && (e.key === 'l' || e.key === 'L')) || (e.ctrlKey && (e.key === 'ArrowLeft' || e.key === 'ArrowRight'))) {
        take()
        return
      }
      // Another song playing: the keys are the player's again until this one is back.
      if (!current || e.ctrlKey || e.altKey || e.metaKey) return
      // Enter on a focused button presses it (Save timing, Leave without saving).
      if (e.key === 'Enter' && t instanceof HTMLButtonElement) return
      if (e.key === ' ' || e.key === 'Enter') {
        take()
        if (!e.repeat) tap()
      } else if (e.key === 'Backspace') {
        take()
        stepBack()
      } else if (e.key === 'b' || e.key === 'B') {
        take()
        addBreak()
      } else if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
        take()
        nudge((e.key === 'ArrowLeft' ? -1 : 1) * (e.shiftKey ? NUDGE_BIG_MS : NUDGE_MS))
      } else if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
        take()
        setSel(Math.min(rows.length - 1, Math.max(0, (sel < 0 ? 0 : sel) + (e.key === 'ArrowUp' ? -1 : 1))))
      } else if (e.key === 'r' || e.key === 'R') {
        take()
        if (sel >= 0) playFrom(sel)
      }
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // The next line to tap (or the one chosen) stays in view.
  const focus = sel >= 0 ? sel : cursor
  useEffect(() => {
    const el = box.current?.querySelector<HTMLElement>(`[data-row="${Math.min(focus, rows.length - 1)}"]`)
    if (el && box.current) box.current.scrollTo({ top: el.offsetTop - box.current.clientHeight / 2 + el.offsetHeight / 2, behavior: 'smooth' })
  }, [focus, rows.length])

  async function save() {
    setSaving(true)
    setError('')
    try {
      // Word times and translations go too: the engine moves the words with their line.
      const lines = rows.map((r) => ({
        ms: r.ms ?? 0,
        text: r.text,
        from_ms: r.from ?? null,
        words: (r.words ?? []).map(([start_ms, end_ms, text]) => ({ start_ms, end_ms, text })),
        translation: r.translation ?? null,
      }))
      const journal = await invoke<string>('save_synced_lyrics', { root: props.root, path: props.path, lines })
      lyricsChanged(props.root, props.path)
      notify('Lyrics timed. They follow the song now.', {
        label: 'Undo',
        run: () =>
          void undoEdit(journal).then(
            () => {
              lyricsChanged(props.root, props.path)
              notify('Timing undone.')
            },
            (e) => notify(`Couldn't undo the timing: ${String(e)}`),
          ),
      })
      onDirty?.(false)
      onClose()
    } catch (e) {
      setError(`Couldn't save the timing: ${String(e)}`)
      setSaving(false)
    }
  }

  // No words yet: paste them first.
  if (!rows.length)
    return (
      <div className="flex h-full flex-col gap-3 py-4">
        <p className="text-[13.5px] text-ink/70">Paste the words, one line per row. Then play the song and tap along to time them.</p>
        <textarea
          autoFocus
          value={pasted}
          onChange={(e) => setPasted(e.target.value)}
          placeholder="The words of the song"
          className="field min-h-0 flex-1 resize-none rounded-2xl p-4 text-[15px] leading-relaxed"
        />
        <div className="flex gap-2">
          <button
            disabled={!pasted.trim()}
            onClick={() => {
              setRows(rowsFrom({ lines: [], text: pasted }))
              setCursor(0)
            }}
            className="btn btn-primary"
          >
            Start timing
          </button>
          <button onClick={onClose} className="btn btn-ghost">
            Cancel
          </button>
        </div>
      </div>
    )

  const left = rows.length - tapped
  const pill = 'flex h-8 items-center gap-1.5 rounded-full px-3 text-[13px] font-medium text-ink/70 transition-colors hover:bg-ink/[0.08] hover:text-ink disabled:pointer-events-none disabled:opacity-35'
  return (
    <div className="flex h-full min-h-0 flex-col pt-4">
      {/* What to do now, and the way out: save once every line has its time. */}
      <div className="flex items-start gap-4">
        <p className="min-w-0 flex-1 text-[13px] leading-relaxed text-ink/65">
          {!playing && cursor === 0
            ? 'Press Space to start the song, then again as each line begins.'
            : cursor < rows.length
              ? 'Press Space as each line begins.'
              : 'Every line has its time. Choose one to nudge it, or press R to hear it again.'}
        </p>
        <div className="flex shrink-0 items-center gap-2">
          {leaving ? (
            <button onClick={onClose} className="btn btn-sm btn-danger">
              Leave without saving
            </button>
          ) : (
            <button onClick={() => (changed.current ? setLeaving(true) : onClose())} className="btn btn-sm btn-ghost">
              Cancel
            </button>
          )}
          <button
            onClick={() => void save()}
            disabled={!!left || saving || !changed.current}
            title={left ? `${left} ${left === 1 ? 'line' : 'lines'} still to tap` : undefined}
            className="btn btn-sm btn-primary"
          >
            {saving ? 'Saving…' : 'Save timing'}
          </button>
        </div>
      </div>
      <div className="mt-3 flex items-center gap-3">
        <div className="h-1 flex-1 overflow-hidden rounded-full bg-ink/10">
          <motion.div
            className="h-full rounded-full"
            style={{ background: left ? 'var(--accent)' : 'var(--color-good)' }}
            animate={{ width: `${(tapped / rows.length) * 100}%` }}
            transition={{ type: 'spring', stiffness: 200, damping: 30 }}
          />
        </div>
        <span className="text-[12px] tabular-nums text-ink/55">{left ? `${tapped} of ${rows.length}` : 'All timed'}</span>
      </div>
      {error && <p className="mt-2 text-[12.5px] text-problem">{error}</p>}
      {!props.current && (
        <div className="mt-3 flex items-center gap-3 rounded-2xl bg-ink/[0.07] px-4 py-2.5 text-[13px]">
          <span className="min-w-0 flex-1 text-ink/70">Another song is playing. Your taps here are kept.</span>
          <button onClick={props.onBack} className="btn btn-sm btn-primary max-w-[50%] truncate">
            Back to {props.title}
          </button>
        </div>
      )}

      <ol
        ref={box}
        className="no-scrollbar relative mt-2 min-h-0 flex-1 overflow-y-auto py-[18vh]"
        style={{ maskImage: 'linear-gradient(transparent, black 16%, black 84%, transparent)' }}
      >
        {rows.map((r, i) => {
          const next = i === cursor
          const chosen = i === sel
          return (
            <li key={i} data-row={i}>
              <button
                onClick={() => playFrom(i)}
                title={r.ms != null ? 'Play from this line' : undefined}
                className={`flex w-full items-baseline gap-4 rounded-xl px-3 py-1.5 text-left transition-colors ${
                  chosen ? 'bg-ink/[0.09]' : 'hover:bg-ink/[0.05]'
                }`}
              >
                <span
                  className={`w-[4.5em] shrink-0 text-right text-[12.5px] tabular-nums ${
                    r.ms == null ? 'text-ink/30' : chosen ? 'text-accent-ink' : 'text-ink/55'
                  }`}
                >
                  {stamp(r.ms)}
                </span>
                <span
                  className={`min-w-0 flex-1 text-[18px] font-semibold leading-snug tracking-[-0.01em] ${
                    i === singing ? 'text-ink' : next ? 'text-ink/85' : r.ms == null ? 'text-ink/40' : 'text-ink/55'
                  }`}
                >
                  {r.text || <span className="text-[14px] font-normal italic text-ink/45">Break</span>}
                </span>
                {next && <span className="shrink-0 rounded-full bg-accent px-2 py-0.5 text-[11px] font-semibold text-on-accent">Next</span>}
              </button>
            </li>
          )
        })}
      </ol>

      {/* The tapping bar, in the tabs' own material. */}
      <div className="flex justify-center pb-1 pt-3">
        <div role="toolbar" aria-label="Timing" className="seg inline-flex flex-wrap items-center justify-center gap-0.5 rounded-full bg-ink/[0.07] p-1 shadow-[inset_0_0_0_1px_var(--edge)]">
          <button
            onClick={tap}
            disabled={cursor >= rows.length && playing}
            className="accent-fill flex h-9 items-center gap-2 rounded-full pl-4 pr-1.5 text-[13.5px] font-semibold transition-transform active:scale-95 disabled:opacity-40"
          >
            {playing ? 'Tap' : 'Play'}
            <kbd className="!border-0 !bg-black/15 !text-current">Space</kbd>
          </button>
          <button onClick={stepBack} disabled={cursor === 0} className={pill} title="Untap the last line and hear it again">
            Step back <kbd>⌫</kbd>
          </button>
          <button onClick={addBreak} disabled={!playing || cursor === 0} className={pill} title="The singing stops here (a long instrumental)">
            Break <kbd>B</kbd>
          </button>
          <span className="mx-1 h-4 w-px bg-ink/15" aria-hidden />
          <button onClick={() => nudge(-NUDGE_MS)} disabled={tapped === 0} className={`${pill} px-2.5`} aria-label="Earlier by a tenth of a second" title="Earlier (←)">
            −
          </button>
          <span className="px-0.5 text-[12.5px] tabular-nums text-ink/55">0.1 s</span>
          <button onClick={() => nudge(NUDGE_MS)} disabled={tapped === 0} className={`${pill} px-2.5`} aria-label="Later by a tenth of a second" title="Later (→)">
            +
          </button>
        </div>
      </div>
      {fixing && !changed.current && (
        <p className="mt-1 text-center text-[12px] text-ink/50">Choose a line and nudge it with ← →, or press Backspace to tap the last lines again.</p>
      )}
    </div>
  )
}
