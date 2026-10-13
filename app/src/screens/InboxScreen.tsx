import { AnimatePresence, motion } from 'motion/react'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { createPortal } from 'react-dom'
import {
  answerDecisions,
  buildLibrary,
  cancelApply,
  inApp,
  review,
  setDecisions,
  type AlbumView,
  type AnswerItem,
  type ApplyReport,
  type PlanDecision,
  type Review,
} from '../lib/engine'
import { loadCovers, useCovers } from '../lib/covers'
import { clipsAvailable, playClips, stopClips, usePlaying } from '../lib/preview'
import { AlbumRow, BuildResult } from '../components/AlbumRow'
import { IconChevron } from '../components/Icons'
import { DeepProgressBar } from './LibraryScreen'
import { IncomingScreen } from './IncomingScreen'
import { Page, Segmented } from '../components/ui'
import { LibraryTidy } from './LibraryTidy'
import { ArtistNames } from '../components/ArtistNames'
import { track, useJob } from '../lib/jobs'

const spring = { type: 'spring', stiffness: 380, damping: 34 } as const

const KIND_LABEL: Record<PlanDecision['kind'], string> = {
  duplicate_songs: 'Same songs',
  same_artist: 'Artist name',
  same_album: 'Same album twice',
  year: 'Original year',
  untagged: 'Files without tags',
}

const plural = (n: number, one: string, many: string) => `${n.toLocaleString('en-IN')} ${n === 1 ? one : many}`

/** The answer item for a choice; "same artist?" also carries both names for your aliases. */
function itemFor(d: PlanDecision, choice: string): AnswerItem {
  return d.kind === 'same_artist' && !d.auto && d.sides.length >= 2
    ? { id: d.id, choice, a: d.sides[0].title, b: d.sides[1].title }
    : { id: d.id, choice }
}

const norm = (p: string) => p.replace(/[\\/]+/g, '/').replace(/\/$/, '').toLowerCase()
const folderName = (p: string) => p.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || p

/**
 * Everything waiting for you: questions about the music you are bringing in, what was handled
 * for you, the build, and new music. New music always goes into your library, never into the
 * folder you are bringing in.
 */
export function InboxScreen(props: {
  root: string
  /** Your library: where the build goes and where new music comes in. */
  home: string | null
  onChooseMusic: () => void
  onImported: () => void
  onCount: (n: number) => void
  onSwitchLibrary: (path: string) => void
}) {
  // Inside your library there is nothing to bring in or build: only new music to add.
  const inLibrary = !!props.home && norm(props.root) === norm(props.home)
  const [tab, setTab] = useState<'library' | 'new'>('library')
  const [data, setData] = useState<Review | null>(null)
  const [error, setError] = useState('')
  const [saving, setSaving] = useState(false)
  const { onCount } = props

  const take = useCallback(
    (r: Review) => {
      setData(r)
      const waiting = r.plan.decisions.filter((d) => !d.answer && !d.auto).length
      const handled = r.plan.decisions.filter((d) => d.auto && !d.answer).length
      onCount(waiting + handled)
      const keys = new Set<string>()
      for (const a of r.plan.albums) keys.add(a.cover)
      for (const d of r.plan.decisions) for (const s of d.sides) s.covers.forEach((c) => keys.add(c))
      void loadCovers(props.root, [...keys], 320)
    },
    [onCount, props.root],
  )

  useEffect(() => {
    if (!props.root || inLibrary) return
    review(props.root)
      .then(take)
      .catch((e) => setError(e instanceof Error ? e.message : String(e)))
  }, [props.root, take, inLibrary])

  const answer = useCallback(
    async (items: AnswerItem[]) => {
      if (!items.length) return
      setSaving(true)
      try {
        take(await answerDecisions(props.root, items))
        setError('')
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e))
      } finally {
        setSaving(false)
      }
    },
    [props.root, take],
  )

  const questions = useMemo(() => data?.plan.decisions.filter((d) => !d.auto && !d.answer) ?? [], [data])
  const handled = useMemo(() => data?.plan.decisions.filter((d) => d.auto) ?? [], [data])
  const newHandled = handled.filter((d) => !d.answer)

  const acceptAll = useCallback(
    () => answer(questions.filter((d) => d.suggested).map((d) => itemFor(d, d.suggested!))),
    [answer, questions],
  )
  const approveAll = useCallback(() => answer(newHandled.map((d) => ({ id: d.id, choice: 'approved' }))), [answer, newHandled])

  // Ctrl+Enter: accept every recommendation, then approve everything handled.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key === 'Enter') {
        e.preventDefault()
        if (questions.length) void acceptAll()
        else if (newHandled.length) void approveAll()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [acceptAll, approveAll, questions.length, newHandled.length])

  useEffect(() => () => stopClips(), [])

  const summary = inLibrary
    ? tab === 'library'
      ? 'How your library is arranged, and the changes that bring it in line with your rules.'
      : 'Add music you have bought or downloaded; it comes into your library tagged, with covers and lyrics.'
    : data
    ? [
        questions.length ? plural(questions.length, 'question for you', 'questions for you') : 'No questions for you',
        newHandled.length ? `${plural(newHandled.length, 'thing', 'things')} handled for you to approve` : '',
      ]
        .filter(Boolean)
        .join(', ') + '.'
    : 'Working out what each album should look like…'

  return (
    <Page
      title="Organize"
      line={summary}
      className="pb-40"
      actions={
        tab === 'library' &&
        questions.length > 0 && (
          <button onClick={() => void acceptAll()} disabled={saving} className="btn-primary btn-lg">
            Accept all recommendations <kbd className="border-black/20 bg-black/10 text-on-accent">Ctrl Enter</kbd>
          </button>
        )
      }
      bar={
        <Segmented
          tabs
          label="Organize"
          value={tab}
          onChange={setTab}
          options={[
            { value: 'library', label: inLibrary ? 'Your library' : `Bringing in ${folderName(props.root)}` },
            { value: 'new', label: 'New music' },
          ]}
        />
      }
    >

      {error && <p className="mb-4 text-[13px] text-problem">{error}</p>}

      {inLibrary && tab === 'library' ? (
        <LibraryTidy root={props.root} onChanged={props.onImported} />
      ) : tab === 'new' || inLibrary ? (
        props.home ? (
          <IncomingScreen root={props.home} onImported={props.onImported} />
        ) : (
          <p className="py-20 text-center text-[14px] text-ink/55">Choose where your library lives first (Settings, Music folders).</p>
        )
      ) : !data ? (
        <p className="py-20 text-center text-[14px] text-ink/55">{summary}</p>
      ) : (
        <>
          {questions.length > 0 ? (
            <QuestionDeck root={props.root} questions={questions} busy={saving} onAnswer={(d, c) => void answer([itemFor(d, c)])} />
          ) : (
            <div className="glass rounded-[22px] px-6 py-5 text-[14px] text-ink/75">
              Nothing needs your answer. Everything the app could not prove was left as it was.
            </div>
          )}
          <Handled
            root={props.root}
            items={handled}
            busy={saving}
            onApproveAll={() => void approveAll()}
            onApprove={(ids) => void answer(ids.map((id) => ({ id, choice: 'approved' })))}
            onChoose={(d, c) => void answer([{ id: d.id, choice: c }])}
          />
          <Changes root={props.root} data={data} onChanged={setData} />
          <NamesSection onSaved={() => void review(props.root).then(take)} />
          <BuildBar root={props.root} data={data} onSwitch={props.onSwitchLibrary} />
        </>
      )}
    </Page>
  )
}

// --- Questions ---------------------------------------------------------------------------

function QuestionDeck(props: {
  root: string
  questions: PlanDecision[]
  busy: boolean
  onAnswer: (d: PlanDecision, choice: string) => void
}) {
  const [i, setI] = useState(0)
  const n = props.questions.length
  const at = Math.min(i, n - 1)
  const d = props.questions[at]

  const playSides = useCallback(() => {
    playClips(
      props.root,
      d.sides.map((s) => s.songs[0]?.path).filter((p): p is string => !!p),
    )
  }, [d, props.root])

  useEffect(() => {
    const typing = (t: EventTarget | null) => t instanceof HTMLElement && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA')
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.altKey || e.metaKey || typing(e.target) || props.busy) return
      if (/^[1-9]$/.test(e.key)) {
        const c = d.choices[Number(e.key) - 1]
        if (c) {
          e.preventDefault()
          props.onAnswer(d, c.id)
        }
      } else if (e.key === '0') {
        e.preventDefault()
        setI((x) => (x + 1) % n)
      } else if (e.key === 'Enter' && d.suggested) {
        e.preventDefault()
        props.onAnswer(d, d.suggested)
      } else if (e.key === ' ') {
        e.preventDefault()
        playSides()
      } else if (e.key === 'ArrowRight') {
        setI((x) => Math.min(x + 1, n - 1))
      } else if (e.key === 'ArrowLeft') {
        setI((x) => Math.max(x - 1, 0))
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [d, n, props, playSides])

  return (
    <section aria-label="Questions for you">
      <div className="mb-3 flex items-center gap-3">
        <h2 className="text-[15px] font-medium text-ink/80">Questions for you</h2>
        <span className="text-[13px] text-ink/50">
          {at + 1} of {n}
        </span>
        <div className="ml-2 flex gap-1" aria-hidden>
          {props.questions.map((q, k) => (
            <span key={q.id} className="h-1 w-5 rounded-full" style={{ background: k === at ? 'var(--accent)' : 'color-mix(in oklab, var(--ink) 15%, transparent)' }} />
          ))}
        </div>
        <div className="ml-auto flex gap-1">
          <button onClick={() => setI(Math.max(at - 1, 0))} disabled={at === 0} aria-label="Previous question" className="rounded-full p-1.5 text-ink/60 hover:bg-ink/10 disabled:opacity-30">
            <IconChevron className="rotate-180" />
          </button>
          <button onClick={() => setI(Math.min(at + 1, n - 1))} disabled={at === n - 1} aria-label="Next question" className="rounded-full p-1.5 text-ink/60 hover:bg-ink/10 disabled:opacity-30">
            <IconChevron />
          </button>
        </div>
      </div>
      <AnimatePresence mode="wait">
        <motion.article
          key={d.id}
          initial={{ opacity: 0, x: 24 }}
          animate={{ opacity: 1, x: 0 }}
          exit={{ opacity: 0, x: -24 }}
          transition={{ duration: 0.2 }}
          className="glass rounded-[24px] p-6"
        >
          <div className="text-[13px] font-medium text-ink/60">{d.question}</div>
          <h3 className="mt-1 text-[24px] font-semibold leading-tight tracking-[-0.02em]">{d.subject}</h3>
          {d.albums.length > 0 && (
            <p className="mt-1 text-[13px] text-ink/55">Holds back {plural(d.albums.length, 'album', 'albums')} until you answer.</p>
          )}
          {d.sides.length > 0 && (
            <div className={`mt-5 grid gap-3 ${d.sides.length >= 3 ? 'md:grid-cols-3' : 'md:grid-cols-2'}`}>
              {d.sides.map((s) => (
                <SideCard key={s.title} root={props.root} side={s} suggested={!!s.choice && s.choice === d.suggested} />
              ))}
            </div>
          )}
          {d.suggested && (
            <div className="mt-4 rounded-2xl px-4 py-3 text-[13.5px]" style={{ background: 'color-mix(in oklab, var(--accent) 14%, transparent)' }}>
              <span className="font-semibold text-accent-ink">
                Recommended: {d.choices.find((c) => c.id === d.suggested)?.label}
                {d.choices.filter((c) => c.label === d.choices.find((x) => x.id === d.suggested)?.label).length > 1
                  ? ` (${d.choices.find((c) => c.id === d.suggested)?.hint})`
                  : ''}
                .
              </span>{' '}
              <span className="text-ink/75">{d.why}</span>
              {clipsAvailable() && d.sides.some((s) => s.songs.length) && <span className="text-ink/55"> Press Space to hear each side.</span>}
            </div>
          )}
          <div className="mt-4 flex flex-wrap gap-2">
            {d.choices.map((c, k) => {
              const rec = c.id === d.suggested
              return (
                <motion.button
                  key={c.id}
                  whileTap={{ scale: 0.97 }}
                  disabled={props.busy}
                  onClick={() => props.onAnswer(d, c.id)}
                  title={c.hint}
                  className={`flex items-center gap-2 rounded-full px-4 py-2 text-[13.5px] disabled:opacity-50 ${
                    rec ? 'bg-accent font-medium text-on-accent' : 'bg-ink/[0.08] hover:bg-ink/[0.14]'
                  }`}
                >
                  <kbd className={rec ? 'border-black/20 bg-black/10 text-on-accent' : ''}>{k + 1}</kbd>
                  {c.label}
                  {d.choices.filter((x) => x.label === c.label).length > 1 && <span className="opacity-70">· {c.hint}</span>}
                </motion.button>
              )
            })}
            <button onClick={() => setI((x) => (x + 1) % n)} className="flex items-center gap-2 rounded-full px-4 py-2 text-[13.5px] text-ink/60 hover:bg-ink/10">
              <kbd>0</kbd> Skip for now
            </button>
          </div>
        </motion.article>
      </AnimatePresence>
    </section>
  )
}

function SideCard(props: { root: string; side: PlanDecision['sides'][number]; suggested: boolean }) {
  const cover = useCovers(320)
  const playing = usePlaying()
  const s = props.side
  return (
    <div
      className="min-w-0 rounded-2xl bg-ink/[0.05] p-4"
      style={{ boxShadow: props.suggested ? 'inset 0 0 0 1.5px color-mix(in oklab, var(--accent) 60%, transparent)' : undefined }}
    >
      {s.covers.length > 0 && (
        <div className="mb-3 flex gap-1.5">
          {s.covers.slice(0, 4).map((k) => {
            const c = cover(k)
            return (
              <div key={k} className="h-14 w-14 shrink-0 overflow-hidden rounded-lg bg-ink/[0.08]">
                {c && <img src={c.src} alt="" draggable={false} className="h-full w-full object-cover" />}
              </div>
            )
          })}
        </div>
      )}
      <div className="truncate text-[16px] font-semibold">{s.title}</div>
      <div className="text-[12.5px] text-ink/55">{s.subtitle}</div>
      {s.facts.length > 0 && (
        <dl className="mt-3 grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 text-[12.5px]">
          {s.facts.map((f) => (
            <div key={f.label} className="contents">
              <dt className="text-ink/50">{f.label}</dt>
              <dd className="m-0 truncate" title={f.value}>
                {f.value}
              </dd>
            </div>
          ))}
        </dl>
      )}
      {s.songs.length > 0 && (
        <div className="mt-3 grid gap-1">
          {s.songs.slice(0, 4).map((song) => {
            const on = playing === song.path
            return (
              <button
                key={song.path}
                disabled={!clipsAvailable()}
                onClick={() => (on ? stopClips() : playClips(props.root, [song.path]))}
                className="flex items-center gap-2 rounded-lg px-1.5 py-1 text-left text-[12.5px] hover:bg-ink/[0.06] disabled:cursor-default disabled:hover:bg-transparent"
                title={clipsAvailable() ? (on ? 'Stop' : 'Play a clip') : 'Clips play in the desktop app'}
              >
                <span className="grid h-6 w-6 shrink-0 place-items-center rounded-full bg-ink/[0.1]">
                  {on ? <span className="h-2 w-2 rounded-[2px] bg-ink" /> : <span className="ml-0.5 border-y-[4.5px] border-l-[7px] border-y-transparent border-l-ink" />}
                </span>
                <span className="min-w-0 flex-1 truncate">{song.title}</span>
                <span className="tabular-nums text-ink/45">{fmtTime(song.duration_ms)}</span>
              </button>
            )
          })}
        </div>
      )}
    </div>
  )
}

const fmtTime = (ms: number) => {
  if (!ms) return ""
  const s = Math.round(ms / 1000)
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`
}

// --- Handled for you ---------------------------------------------------------------------

function Handled(props: {
  root: string
  items: PlanDecision[]
  busy: boolean
  onApproveAll: () => void
  onApprove: (ids: string[]) => void
  onChoose: (d: PlanDecision, choice: string) => void
}) {
  const [open, setOpen] = useState<string | null>(null)
  const [showApproved, setShowApproved] = useState(false)
  const [expanded, setExpanded] = useState<Set<string>>(new Set())
  const fresh = props.items.filter((d) => !d.answer)
  const approved = props.items.filter((d) => d.answer)
  if (!props.items.length) return null
  const row = (d: PlanDecision) => {
    const isOpen = open === d.id
    const current = d.answer && d.answer !== 'approved' ? d.answer : d.suggested
    return (
      <motion.div layout transition={spring} key={d.id} className="glass overflow-hidden rounded-[18px]">
        <button onClick={() => setOpen(isOpen ? null : d.id)} aria-expanded={isOpen} className="flex w-full items-start gap-4 px-5 py-3.5 text-left">
          <span className="mt-0.5 shrink-0 rounded-full bg-ink/[0.08] px-2.5 py-0.5 text-[11.5px] text-ink/70">{KIND_LABEL[d.kind]}</span>
          <span className="min-w-0 flex-1">
            <span className="block text-[14.5px] font-medium">{d.subject}</span>
            <span className="mt-0.5 block text-[12.5px] leading-relaxed text-good">{d.proof}</span>
          </span>
          {d.answer && <span className="mt-0.5 shrink-0 text-[12px] text-ink/45">{d.answer === 'approved' ? 'Approved' : 'Changed by you'}</span>}
          <motion.span animate={{ rotate: isOpen ? 90 : 0 }} transition={spring} className="mt-0.5 text-ink/45">
            <IconChevron />
          </motion.span>
        </button>
        <AnimatePresence initial={false}>
          {isOpen && (
            <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} className="border-t border-edge px-5 pb-5 pt-4">
              {d.sides.length > 0 && (
                <div className={`grid gap-3 ${d.sides.length >= 3 ? 'md:grid-cols-3' : 'md:grid-cols-2'}`}>
                  {d.sides.map((s) => (
                    <SideCard key={s.title} root={props.root} side={s} suggested={!!s.choice && s.choice === current} />
                  ))}
                </div>
              )}
              <p className="mt-3 text-[13px] text-ink/65">{d.why}</p>
              <div className="mt-3 flex flex-wrap gap-2">
                {d.choices.map((c) => {
                  const on = c.id === current
                  return (
                    <button
                      key={c.id}
                      disabled={props.busy || on}
                      onClick={() => props.onChoose(d, c.id)}
                      title={c.hint}
                      className={`rounded-full px-4 py-1.5 text-[13px] ${on ? 'bg-accent text-on-accent' : 'bg-ink/[0.08] hover:bg-ink/[0.14]'}`}
                    >
                      {on ? '✓ ' : ''}
                      {c.label}
                    </button>
                  )
                })}
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </motion.div>
    )
  }
  return (
    <section aria-label="Handled for you" className="mt-10">
      <div className="mb-3 flex flex-wrap items-center gap-3">
        <h2 className="text-[15px] font-medium text-ink/80">Handled for you</h2>
        <span className="text-[13px] text-ink/50">Applied already, each with its proof. Open one to change it.</span>
        {fresh.length > 0 && (
          <button
            onClick={props.onApproveAll}
            disabled={props.busy}
            className="ml-auto flex items-center gap-2 rounded-full bg-ink/[0.1] px-4 py-1.5 text-[13px] hover:bg-ink/[0.16] disabled:opacity-50"
          >
            Approve all {fresh.length}
          </button>
        )}
      </div>
      <div className="grid gap-5">
        {(Object.keys(KIND_LABEL) as PlanDecision['kind'][])
          .map((k) => ({ k, list: fresh.filter((d) => d.kind === k) }))
          .filter((g) => g.list.length)
          .map(({ k, list }) => {
            const all = expanded.has(k)
            return (
              <div key={k}>
                <div className="mb-2 flex items-center gap-3 px-1">
                  <span className="text-[13.5px] font-medium">{KIND_LABEL[k]}</span>
                  <span className="text-[12.5px] text-ink/50">{list.length}</span>
                  <button
                    onClick={() => props.onApprove(list.map((d) => d.id))}
                    disabled={props.busy}
                    className="ml-auto rounded-full px-3 py-1 text-[12.5px] text-ink/70 hover:bg-ink/10 disabled:opacity-50"
                  >
                    Approve these {list.length}
                  </button>
                </div>
                <div className="grid gap-2">{(all ? list : list.slice(0, 3)).map(row)}</div>
                {list.length > 3 && (
                  <button
                    onClick={() => setExpanded((s) => {
                      const n = new Set(s)
                      if (all) n.delete(k)
                      else n.add(k)
                      return n
                    })}
                    className="mt-2 px-1 text-[12.5px] text-ink/55 hover:text-ink"
                  >
                    {all ? 'Show fewer' : `Show all ${list.length}`}
                  </button>
                )}
              </div>
            )
          })}
      </div>
      {approved.length > 0 && (
        <div className="mt-3">
          <button onClick={() => setShowApproved(!showApproved)} className="text-[13px] text-ink/55 hover:text-ink">
            {showApproved ? 'Hide' : 'Show'} {plural(approved.length, 'approved item', 'approved items')}
          </button>
          {showApproved && <div className="mt-2 grid gap-2">{approved.map(row)}</div>}
        </div>
      )}
    </section>
  )
}

// --- What the build changes --------------------------------------------------------------

interface Story {
  id: string
  label: string
  test: (a: AlbumView) => boolean
}

const has = (a: AlbumView, ...fields: string[]) => a.changes.some((c) => fields.includes(c.field.toUpperCase()))

const STORIES: Story[] = [
  { id: 'covers', label: 'get a sharper cover', test: (a) => !!a.cover_upgrade },
  { id: 'lyrics', label: 'get synced lyrics', test: (a) => has(a, 'LYRICS') },
  { id: 'names', label: 'have artist names made consistent', test: (a) => has(a, 'ALBUMARTIST', 'ARTIST') },
  { id: 'credits', label: 'get composer or lyricist credits', test: (a) => has(a, 'COMPOSER', 'LYRICIST') },
  { id: 'years', label: 'get their original release year', test: (a) => has(a, 'ORIGINALYEAR') },
  { id: 'genre', label: 'get genre, language and category', test: (a) => has(a, 'GENRE', 'LANGUAGE', 'CATEGORY') },
  { id: 'loudness', label: 'get volume levelling', test: (a) => has(a, 'REPLAYGAIN_TRACK_GAIN') },
  { id: 'titles', label: 'have album or song titles tidied', test: (a) => has(a, 'ALBUM', 'TITLE') },
  { id: 'tidy', label: 'lose leftover junk in their tags', test: (a) => a.changes.some((c) => c.after.length === 0 && c.before.length > 0) },
]

function Changes(props: { root: string; data: Review; onChanged: (r: Review) => void }) {
  const cover = useCovers(320)
  const [story, setStory] = useState<string | null>(null)
  const [album, setAlbum] = useState<AlbumView | null>(null)
  const albums = props.data.plan.albums
  const ready = albums.filter((a) => a.action.kind === 'migrate' && a.waiting_on.length === 0 && props.data.decisions[a.id] !== 'skipped')
  const setAside = albums.filter((a) => a.action.kind === 'quarantine')
  const waiting = albums.filter((a) => a.waiting_on.length > 0)
  const leftOut = albums.filter((a) => props.data.decisions[a.id] === 'skipped')
  const groups = [
    ...STORIES.map((s) => ({ id: s.id, label: s.label, list: ready.filter(s.test) })),
    { id: 'aside', label: 'are copies, set aside', list: setAside },
    { id: 'waiting', label: 'wait for your answers', list: waiting },
    { id: 'out', label: 'are left out by you', list: leftOut },
  ].filter((g) => g.list.length)
  const shown = groups.find((g) => g.id === story)

  async function leaveOut(a: AlbumView, out: boolean) {
    const d = await setDecisions(props.root, { [a.id]: out ? 'skipped' : null })
    props.onChanged({ ...props.data, decisions: d })
  }

  return (
    <section aria-label="What the build changes" className="mt-10">
      <div className="mb-3 flex flex-wrap items-baseline gap-3">
        <h2 className="text-[15px] font-medium text-ink/80">What the build changes</h2>
        <span className="text-[13px] text-ink/50">
          {plural(ready.length, 'album', 'albums')} ready. Open a story to see the albums and the exact changes.
        </span>
      </div>
      <div className="grid grid-cols-[repeat(auto-fill,minmax(200px,1fr))] gap-2.5">
        {groups.map((g) => {
          const on = story === g.id
          return (
            <button
              key={g.id}
              onClick={() => setStory(on ? null : g.id)}
              aria-pressed={on}
              className="glass rounded-[18px] p-4 text-left transition-transform hover:-translate-y-0.5"
              style={{ boxShadow: on ? 'inset 0 0 0 1.5px var(--accent)' : undefined }}
            >
              <div className="text-[26px] font-semibold leading-none tracking-[-0.02em]">{g.list.length.toLocaleString('en-IN')}</div>
              <div className="mt-1.5 text-[13px] leading-snug text-ink/65">albums {g.label}</div>
              <div className="mt-3 flex -space-x-2">
                {g.list.slice(0, 5).map((a) => {
                  const c = cover(a.cover)
                  return (
                    <div key={a.id} className="h-8 w-8 overflow-hidden rounded-md bg-ink/[0.1] ring-2 ring-[var(--app-bg)]">
                      {c && <img src={c.src} alt="" className="h-full w-full object-cover" />}
                    </div>
                  )
                })}
              </div>
            </button>
          )
        })}
      </div>
      <AnimatePresence>
        {shown && (
          <motion.div
            key={shown.id}
            initial={{ opacity: 0, y: 8 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0 }}
            className="mt-4 grid grid-cols-[repeat(auto-fill,minmax(120px,1fr))] gap-x-4 gap-y-5"
          >
            {shown.list.map((a) => {
              const c = cover(a.cover)
              return (
                <button key={a.id} onClick={() => setAlbum(a)} className="group text-left">
                  <div className="aspect-square overflow-hidden rounded-xl bg-ink/[0.06] shadow-lg shadow-black/25">
                    {c && <img src={c.src} alt="" loading="lazy" className="h-full w-full object-cover transition-transform group-hover:scale-[1.04]" />}
                  </div>
                  <div className="mt-1.5 truncate text-[12.5px] font-medium">{a.title}</div>
                  <div className="truncate text-[11.5px] text-ink/50">{a.album_artist}</div>
                </button>
              )
            })}
          </motion.div>
        )}
      </AnimatePresence>
      {createPortal(
        <AnimatePresence>
          {album && (
            <motion.div
              className="fixed inset-0 z-40 grid place-items-center bg-black/40 p-8"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              onMouseDown={() => setAlbum(null)}
              onKeyDown={(e) => e.key === 'Escape' && setAlbum(null)}
            >
              <motion.div
                className="no-scrollbar max-h-[86vh] w-[min(880px,100%)] overflow-y-auto"
                initial={{ scale: 0.97 }}
                animate={{ scale: 1 }}
                exit={{ scale: 0.97 }}
                onMouseDown={(e) => e.stopPropagation()}
              >
                <AlbumRow
                  album={album}
                  open
                  onToggle={() => setAlbum(null)}
                  decision={props.data.decisions[album.id] === 'skipped' ? undefined : 'approved'}
                  approveLabel="In the build"
                  onDecide={(d) => void leaveOut(album, d === null)}
                />
              </motion.div>
            </motion.div>
          )}
        </AnimatePresence>,
        document.body,
      )}
    </section>
  )
}

// --- Build ---------------------------------------------------------------------------------

function BuildBar(props: { root: string; data: Review; onSwitch: (path: string) => void }) {
  const [mine, setBuilding] = useState<{ done: number; total: number } | null>(null)
  // A build keeps going when you leave Organize; coming back picks its progress up again.
  const running = useJob('build')
  const building = mine ?? (running ? { done: running.done, total: running.total } : null)
  const [result, setResult] = useState<ApplyReport | null>(null)
  const [error, setError] = useState('')
  const albums = props.data.plan.albums
  const ready = albums.filter((a) => a.waiting_on.length === 0 && props.data.decisions[a.id] !== 'skipped')
  const copies = ready.filter((a) => a.action.kind === 'quarantine').length
  const waiting = albums.filter((a) => a.waiting_on.length > 0).length
  const target = props.data.settings.new_library

  async function build() {
    setError('')
    setResult(null)
    setBuilding({ done: 0, total: 0 })
    try {
      setResult(
        await track({ id: 'build', label: 'Building your library', unit: 'songs', cancel: () => void cancelApply() }, (update) =>
          buildLibrary(props.root, (done, total) => {
            setBuilding({ done, total })
            update(done, total)
          }),
        ),
      )
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBuilding(null)
    }
  }

  return (
    <section aria-label="Build" className="mt-10">
      {result && <BuildResult report={result} onDismiss={() => setResult(null)} />}
      {result && target && result.tracks_written + result.tracks_skipped > 0 && (
        <div className="glass mt-3 flex flex-wrap items-center gap-4 rounded-[22px] px-6 py-4">
          <div className="min-w-0 flex-1 text-[13.5px] text-ink/75">
            Your new library is ready in {target}. Switch to it to browse, sync cards and add new music there from now on. You can
            switch back any time in Settings.
          </div>
          <button onClick={() => props.onSwitch(target)} className="rounded-full bg-accent px-5 py-2 text-[13.5px] font-medium text-on-accent">
            Switch to the new library
          </button>
        </div>
      )}
      <AnimatePresence>
        {building && <DeepProgressBar deep={building} label="Building your library" unit="songs" onCancel={() => void cancelApply()} />}
      </AnimatePresence>
      <div className="glass flex flex-wrap items-center gap-4 rounded-[22px] px-6 py-5">
        <div className="min-w-0 flex-1">
          <div className="text-[15px] font-medium">Build your library</div>
          <div className="mt-0.5 text-[12.5px] leading-relaxed text-ink/55">
            {plural(ready.length - copies, 'album', 'albums')} into {target ?? 'a folder you choose in Settings'}
            {copies ? `, and ${plural(copies, 'copy', 'copies')} set aside in _Quarantine` : ''}.
            {waiting ? ` ${plural(waiting, 'album waits', 'albums wait')} for your answers and will be built later.` : ''} Your
            music folder is only read, never changed.
          </div>
          {!target && (
            <div className="mt-1 text-[12.5px] text-fix">Choose where your library lives first, in Settings, Music folders.</div>
          )}
          {target && !ready.length && waiting > 0 && (
            <div className="mt-1 text-[12.5px] text-fix">Answer the questions above, and those albums are ready to build.</div>
          )}
          {error && <div className="mt-1 text-[12.5px] text-problem">{error}</div>}
        </div>
        <button
          onClick={() => void build()}
          disabled={!inApp() || !!building || !ready.length || !target}
          title={inApp() ? '' : 'The library is built by the desktop app'}
          className="rounded-full bg-accent px-6 py-2.5 text-[14px] font-medium text-on-accent disabled:opacity-40"
        >
          {building ? 'Building…' : 'Build library'}
        </button>
      </div>
    </section>
  )
}

/** How artists are written, while bringing music in: the build uses these spellings. */
function NamesSection(props: { onSaved: () => void }) {
  const [open, setOpen] = useState(false)
  return (
    <section aria-label="Artist names" className="glass mt-10 rounded-[22px] px-6 py-5">
      <button onClick={() => setOpen((o) => !o)} aria-expanded={open} className="flex w-full items-center gap-3 text-left">
        <span className="flex-1">
          <span className="block text-[15px] font-medium">How artists are written</span>
          <span className="mt-0.5 block text-[12.5px] text-ink/55">
            One spelling per person everywhere; your answers above add to it. Click a name to change it.
          </span>
        </span>
        <span className={`text-ink/50 transition-transform ${open ? 'rotate-90' : ''}`}>›</span>
      </button>
      {open && (
        <div className="mt-4">
          <ArtistNames onSaved={props.onSaved} />
        </div>
      )}
    </section>
  )
}
