import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useMemo, useRef, useState } from 'react'
import {
  cardChoices,
  cardEstimate,
  gb,
  players,
  setupCard,
  type CardConfig,
  type CardKind,
  type CardSettings,
  type Choice,
  type Choices,
  type Convert,
  type Estimate,
  type Extras,
  type CardPick,
  type PlayerId,
  type PlayerInfo,
} from '../../lib/engine'
import { loadCovers, useCovers } from '../../lib/covers'
import { keyFor, matcher } from '../../lib/textsearch'
import { Segmented, Switch } from '../../components/ui'
import { IconSearch } from '../../components/Icons'
import { PillButton } from '../libraryParts'

type Tab = 'playlists' | 'albums' | 'artists'

const KINDS: { value: CardKind; label: string; line: string }[] = [
  { value: 'player', label: 'For my player', line: 'Written the way your player reads it.' },
  { value: 'gift', label: 'A gift', line: 'For someone else: with a README, and a note of what is missing so they can build on it.' },
  { value: 'car', label: 'Car or USB stick', line: 'Folders to browse, in a format every car stereo plays.' },
]

// The choices are the same for every card of a library: read them once.
let kept: { root: string; choices: Promise<Choices> } | null = null
function choicesFor(root: string) {
  if (kept?.root !== root) kept = { root, choices: cardChoices(root) }
  return kept.choices
}

const emptyPick = (): CardPick => ({ playlists: [], albums: [], artists: [] })
const picked = (p: CardPick) => p.playlists.length + p.albums.length + p.artists.length

/** Set up a card, or change what it carries and how. */
export function CardEditor(props: {
  root: string
  /** The card's drive. */
  card: string
  initial: CardConfig | null
  defaultName: string
  folders: string[]
  onSaved: () => void
}) {
  const init = props.initial
  const [name, setName] = useState(init?.name ?? props.defaultName)
  const [kind, setKind] = useState<CardKind>(init?.kind ?? 'player')
  const [all, setAll] = useState(!init?.pick || picked(init.pick) === 0)
  const [pick, setPick] = useState<CardPick>(init?.pick ?? emptyPick())
  const [include, setInclude] = useState<string[]>(init?.include ?? [])
  const [player, setPlayer] = useState<PlayerId>(init?.player ?? 'poweramp')
  const [convert, setConvert] = useState<Convert>(init?.convert ?? { hires: false, aac: false })
  const [extras, setExtras] = useState<Extras>(init?.extras ?? { readme: true, missing: true, gaps: true })
  const [auto, setAuto] = useState(init?.auto_sync ?? false)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState('')

  const settings: CardSettings = useMemo(
    () => ({
      name: name.trim() || props.defaultName,
      include: all ? include : [],
      pick: all ? null : pick,
      auto_sync: auto,
      // A gift or a car card isn't for a particular player: exact copies, playlists for all.
      player: kind === 'player' ? player : 'standard',
      kind,
      convert,
      extras,
    }),
    [name, props.defaultName, all, include, pick, auto, kind, player, convert, extras],
  )

  function chooseKind(k: CardKind) {
    setKind(k)
    // A car plays AAC everywhere; FLAC is not a given.
    if (k === 'car' && kind !== 'car') setConvert({ hires: false, aac: true })
  }

  async function save() {
    setSaving(true)
    setError('')
    try {
      await setupCard(props.card, settings)
      props.onSaved()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  const nothingChosen = !all && picked(pick) === 0

  return (
    <div className="mt-4 grid gap-6 border-t border-ink/[0.06] pt-5">
      <div className="flex flex-wrap items-end gap-x-6 gap-y-3">
        <label className="grid w-[300px] max-w-full gap-1">
          <span className="text-[12px] text-ink/55">Name</span>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="rounded-xl bg-ink/[0.06] px-3 py-2 text-[13.5px] outline-none shadow-[inset_0_0_0_1px_var(--edge)] focus:shadow-[inset_0_0_0_1px_color-mix(in_oklab,var(--accent)_60%,transparent)]"
          />
        </label>
        <div className="grid gap-1">
          <span className="text-[12px] text-ink/55">What it's for</span>
          <Segmented label="What this card is for" value={kind} onChange={chooseKind} options={KINDS} />
        </div>
      </div>
      <p className="-mt-3 text-[12.5px] text-ink/55">{KINDS.find((k) => k.value === kind)?.line}</p>

      <section className="grid gap-3">
        <div className="flex flex-wrap items-center gap-4">
          <h3 className="text-[15px] font-semibold tracking-[-0.01em]">What it carries</h3>
          <Segmented
            label="What it carries"
            size="sm"
            value={all ? 'all' : 'pick'}
            onChange={(v) => setAll(v === 'all')}
            options={[
              { value: 'all', label: 'Everything' },
              { value: 'pick', label: 'A selection' },
            ]}
          />
        </div>
        {all ? (
          <FolderChoice folders={props.folders} include={include} onChange={setInclude} />
        ) : (
          <Picker root={props.root} pick={pick} onChange={setPick} />
        )}
        <Planner root={props.root} card={props.card} settings={settings} empty={nothingChosen} />
      </section>

      {kind === 'player' && <PlayerChoice player={player} onChange={setPlayer} />}

      <section className="grid gap-1">
        <h3 className="mb-1 text-[15px] font-semibold tracking-[-0.01em]">Smaller files</h3>
        <Toggle
          label="Hi-res as 16-bit FLAC"
          line="Still lossless, and plays on everything. A 24-bit or 96 kHz song takes about a third of the space."
          on={convert.hires && !convert.aac}
          disabled={convert.aac}
          onChange={(on) => setConvert({ ...convert, hires: on })}
        />
        <Toggle
          label="AAC, for phones and cars"
          line="At 192 kbps, hard to tell from the original by ear, at about a fifth of the size. Made by Windows itself."
          on={convert.aac}
          onChange={(on) => setConvert({ ...convert, aac: on })}
        />
        <p className="mt-1 text-[12px] text-ink/45">Only the card's copies change; your library keeps every file as it is.</p>
      </section>

      {kind === 'gift' && (
        <section className="grid gap-1">
          <h3 className="mb-1 text-[15px] font-semibold tracking-[-0.01em]">With the music</h3>
          <Toggle label="A README" line="What's on the card, who made it, and how to play it." on={extras.readme} onChange={(on) => setExtras({ ...extras, readme: on })} />
          <Toggle
            label="What's missing"
            line="A track list in each album that isn't complete, and one list of every song to look for next."
            on={extras.missing}
            onChange={(on) => setExtras({ ...extras, missing: on })}
          />
          <Toggle
            label="Playlists keep their gaps"
            line="Songs not on the card stay in the playlists as notes players skip, so Venuta can fill them in later."
            on={extras.gaps}
            onChange={(on) => setExtras({ ...extras, gaps: on })}
          />
        </section>
      )}

      <div className="grid gap-3">
        <Toggle label="Sync when inserted" line="While Venuta is open, this card is brought up to date as soon as you put it in." on={auto} onChange={setAuto} />
        <div className="flex items-center gap-3">
          <PillButton onClick={() => void save()} accent disabled={saving || nothingChosen}>
            {saving ? 'Saving…' : 'Save'}
          </PillButton>
          {nothingChosen && <span className="text-[12.5px] text-ink/55">Choose something for the card first.</span>}
          {error && <span className="text-[12.5px] text-problem">{error}</span>}
        </div>
      </div>
    </div>
  )
}

function Toggle(props: { label: string; line: string; on: boolean; disabled?: boolean; onChange: (on: boolean) => void }) {
  return (
    <div className={`flex items-center gap-4 rounded-xl py-2 ${props.disabled ? 'opacity-50' : ''}`}>
      <div className="min-w-0 flex-1">
        <div className="text-[13.5px]">{props.label}</div>
        <div className="text-[12.5px] leading-snug text-ink/55">{props.line}</div>
      </div>
      <Switch on={props.on} onChange={props.onChange} label={props.label} disabled={props.disabled} />
    </div>
  )
}

function FolderChoice(props: { folders: string[]; include: string[]; onChange: (v: string[]) => void }) {
  if (props.folders.length < 2) return <p className="text-[12.5px] text-ink/55">Your whole library, and every playlist.</p>
  return (
    <div>
      <p className="text-[12.5px] text-ink/55">Your whole library and every playlist, or only some of its folders:</p>
      <div className="mt-2 flex flex-wrap gap-2">
        {props.folders.map((f) => {
          const on = props.include.includes(f)
          return (
            <button
              key={f}
              aria-pressed={on}
              onClick={() => props.onChange(on ? props.include.filter((x) => x !== f) : [...props.include, f])}
              className="rounded-full px-3 py-1.5 text-[12.5px]"
              style={{ background: on ? 'color-mix(in oklab, var(--accent) 30%, transparent)' : 'color-mix(in oklab, var(--ink) 7%, transparent)' }}
            >
              {f}
            </button>
          )
        })}
      </div>
    </div>
  )
}

/** Rows shown at once in a list; searching narrows it. */
const SHOWN = 120

function Picker(props: { root: string; pick: CardPick; onChange: (p: CardPick) => void }) {
  const [choices, setChoices] = useState<Choices | null>(null)
  const [tab, setTab] = useState<Tab>('playlists')
  const [q, setQ] = useState('')
  const cover = useCovers(96)
  useEffect(() => {
    let live = true
    choicesFor(props.root)
      .then((c) => live && setChoices(c))
      .catch(() => live && setChoices({ playlists: [], albums: [], artists: [] }))
    return () => {
      live = false
    }
  }, [props.root])

  const shown = useMemo(() => {
    const has = matcher(q)
    return (choices?.[tab] ?? []).filter((c) => has(keyFor(c, () => `${c.name} ${c.detail}`)))
  }, [choices, tab, q])
  // Covers for the albums in view.
  useEffect(() => {
    if (tab === 'albums' && shown.length)
      void loadCovers(
        props.root,
        shown.slice(0, SHOWN).map((c) => c.id),
        96,
      )
  }, [tab, shown, props.root])
  const chosen = new Set(props.pick[tab])
  const toggle = (c: Choice) => {
    const now = props.pick[tab]
    props.onChange({ ...props.pick, [tab]: chosen.has(c.id) ? now.filter((x) => x !== c.id) : [...now, c.id] })
  }
  const names = useMemo(() => {
    const m = new Map<string, string>()
    for (const t of ['playlists', 'albums', 'artists'] as Tab[]) for (const c of choices?.[t] ?? []) m.set(`${t}:${c.id}`, c.name)
    return m
  }, [choices])

  return (
    <div className="glass grid gap-3 rounded-2xl p-4">
      {picked(props.pick) > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {(['playlists', 'albums', 'artists'] as Tab[]).flatMap((t) =>
            props.pick[t].map((id) => (
              <button
                key={`${t}:${id}`}
                onClick={() => props.onChange({ ...props.pick, [t]: props.pick[t].filter((x) => x !== id) })}
                title="Take it off the card"
                className="group flex items-center gap-1.5 rounded-full py-1 pl-3 pr-2 text-[12.5px]"
                style={{ background: 'color-mix(in oklab, var(--accent) 22%, transparent)' }}
              >
                {names.get(`${t}:${id}`) ?? id}
                <span aria-hidden className="text-ink/50 group-hover:text-ink">×</span>
              </button>
            )),
          )}
        </div>
      )}
      <div className="flex flex-wrap items-center gap-3">
        <Segmented
          label="Choose from"
          tabs
          size="sm"
          value={tab}
          onChange={(t) => setTab(t)}
          options={(['playlists', 'albums', 'artists'] as Tab[]).map((t) => ({
            value: t,
            label: (
              <>
                {t[0].toUpperCase() + t.slice(1)}
                {props.pick[t].length > 0 && <span className="ml-1.5 text-ink/55">{props.pick[t].length}</span>}
              </>
            ),
          }))}
        />
        <label className="ml-auto flex min-w-[220px] max-w-[320px] flex-1 items-center gap-2 rounded-full bg-ink/[0.07] px-3.5 shadow-[inset_0_0_0_1px_var(--edge)]">
          <IconSearch className="shrink-0 text-ink/45" />
          <input
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder={`Search ${tab}`}
            aria-label={`Search ${tab}`}
            className="min-w-0 flex-1 bg-transparent py-1.5 text-[13px] outline-none placeholder:text-ink/45"
          />
        </label>
      </div>
      <div className="thin-scroll fade-y -mx-1 max-h-[340px] overflow-y-auto px-1">
        {!choices && <p className="py-6 text-center text-[13px] text-ink/55">Reading your library…</p>}
        {choices && shown.length === 0 && <p className="py-6 text-center text-[13px] text-ink/55">Nothing matches “{q.trim()}”.</p>}
        {shown.slice(0, SHOWN).map((c) => {
          const on = chosen.has(c.id)
          const art = tab === 'albums' ? cover(c.id) : undefined
          return (
            <button
              key={c.id}
              role="checkbox"
              aria-checked={on}
              onClick={() => toggle(c)}
              className="row flex w-full items-center gap-3 rounded-xl px-2 py-1.5 text-left hover:bg-ink/[0.05]"
            >
              <span
                aria-hidden
                className={`grid h-5 w-5 shrink-0 place-items-center rounded-md text-[11px] ${on ? 'accent-fill text-on-accent' : 'shadow-[inset_0_0_0_1.5px_color-mix(in_oklab,var(--ink)_30%,transparent)]'}`}
              >
                {on && '✓'}
              </span>
              {tab === 'albums' && (
                <span className="h-9 w-9 shrink-0 overflow-hidden rounded-md bg-ink/[0.06]">
                  {art && <img src={art.src} alt="" loading="lazy" className="h-full w-full object-cover" />}
                </span>
              )}
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[13.5px]">{c.name}</span>
                {c.detail && <span className="block truncate text-[12px] text-ink/50">{c.detail}</span>}
              </span>
              <span className="shrink-0 text-right text-[12px] tabular-nums text-ink/50">
                {c.songs ? `${c.songs.toLocaleString('en-IN')} ${c.songs === 1 ? 'song' : 'songs'}` : 'None of its songs yet'}
                {c.songs > 0 && <span className="block">{gb(c.bytes)}</span>}
              </span>
            </button>
          )
        })}
        {shown.length > SHOWN && (
          <p className="py-2 text-center text-[12px] text-ink/45">
            {SHOWN} of {shown.length.toLocaleString('en-IN')} shown: search to find the rest.
          </p>
        )}
      </div>
    </div>
  )
}

/** How much the card will hold with these settings, against the room it has. */
function Planner(props: { root: string; card: string; settings: CardSettings; empty: boolean }) {
  const [est, setEst] = useState<Estimate | null>(null)
  const [busy, setBusy] = useState(false)
  const asked = useRef(0)
  const key = JSON.stringify(props.settings)
  useEffect(() => {
    if (props.empty) {
      setEst(null)
      return
    }
    const n = ++asked.current
    setBusy(true)
    const t = window.setTimeout(() => {
      cardEstimate(props.root, props.card, props.settings)
        .then((e) => n === asked.current && setEst(e))
        .catch(() => n === asked.current && setEst(null))
        .finally(() => n === asked.current && setBusy(false))
    }, 350)
    return () => window.clearTimeout(t)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, props.root, props.card, props.empty])

  if (props.empty) return null
  if (!est) return <p className="text-[12.5px] text-ink/55">{busy ? 'Working out the size…' : ''}</p>
  const share = est.room > 0 ? Math.min(1, est.bytes / est.room) : 1
  return (
    <div className={`grid gap-1.5 transition-opacity ${busy ? 'opacity-60' : ''}`}>
      <div className="h-2 overflow-hidden rounded-full bg-ink/[0.08]">
        <motion.div
          className="h-full rounded-full"
          style={{ background: est.fits ? 'var(--accent)' : 'var(--problem)' }}
          animate={{ width: `${Math.max(1, share * 100)}%` }}
          transition={{ type: 'spring', stiffness: 260, damping: 30 }}
        />
      </div>
      <AnimatePresence mode="wait" initial={false}>
        <motion.p key={String(est.fits)} initial={{ opacity: 0 }} animate={{ opacity: 1 }} className="text-[12.5px] text-ink/70">
          {est.songs.toLocaleString('en-IN')} {est.songs === 1 ? 'song' : 'songs'}, {gb(est.bytes)}
          {props.settings.convert.aac || props.settings.convert.hires ? ' (converted sizes are close estimates)' : ''}.{' '}
          {est.fits ? (
            <>
              {gb(est.room)} of room on the card{est.reserve ? `, keeping ${gb(est.reserve)} free` : ''}.
            </>
          ) : (
            <span className="text-problem">
              {gb(est.bytes - est.room)} too much for this card: choose less, or turn on smaller files.
            </span>
          )}
        </motion.p>
      </AnimatePresence>
    </div>
  )
}

function PlayerChoice(props: { player: PlayerId; onChange: (p: PlayerId) => void }) {
  const [list, setList] = useState<PlayerInfo[]>([])
  useEffect(() => {
    players()
      .then(setList)
      .catch(() => setList([]))
  }, [])
  const chosen = list.find((p) => p.id === props.player)
  return (
    <section className="grid gap-2">
      <h3 className="text-[15px] font-semibold tracking-[-0.01em]">The player that reads it</h3>
      <div className="flex flex-wrap gap-2" role="radiogroup" aria-label="Player">
        {list.map((p) => (
          <button
            key={p.id}
            role="radio"
            aria-checked={props.player === p.id}
            onClick={() => props.onChange(p.id)}
            className="rounded-full px-3 py-1.5 text-[12.5px]"
            style={{ background: props.player === p.id ? 'color-mix(in oklab, var(--accent) 30%, transparent)' : 'color-mix(in oklab, var(--ink) 7%, transparent)' }}
          >
            {p.label}
          </button>
        ))}
      </div>
      {chosen && (
        <ul className="grid gap-0.5 text-[12.5px] text-ink/60">
          {chosen.notes.map((n) => (
            <li key={n}>{n}</li>
          ))}
        </ul>
      )}
    </section>
  )
}
