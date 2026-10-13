import { AnimatePresence, motion } from 'motion/react'
import { useCallback, useEffect, useState } from 'react'
import {
  cancelSync,
  ejectCard,
  gb,
  inApp,
  libraryFolders,
  listCards,
  planCard,
  setupCard,
  syncCard,
  type CardStatus,
  type SyncPlan,
  type SyncReport,
  players,
  playerTestRemove,
  playerTestSave,
  playerTestWrite,
  type PlayerId,
  type PlayerInfo,
  type TestItem,
} from '../lib/engine'
import { PillButton } from './LibraryScreen'
import { IconCards } from '../components/Icons'
import { track } from '../lib/jobs'

const PLAYER_NAMES: Record<PlayerId, string> = {
  standard: 'any player (exact copies)',
  poweramp: 'Poweramp',
  snowsky_disc: 'Snowsky Disc',
  echo_mini: 'Echo Mini',
  astell_kern: 'Astell&Kern',
}

const spring = { type: 'spring', stiffness: 320, damping: 30 } as const

type Busy =
  | { kind: 'planning' }
  | { kind: 'plan'; plan: SyncPlan }
  | { kind: 'syncing'; done: number; total: number; bytes: number }
  | { kind: 'done'; report: SyncReport }
  | { kind: 'ejecting' }

export function CardsScreen({ root }: { root: string }) {
  const [cards, setCards] = useState<CardStatus[]>([])
  const [folders, setFolders] = useState<string[]>([])
  const [state, setState] = useState<Record<string, Busy | undefined>>({})
  const [message, setMessage] = useState('')

  const refresh = useCallback(async () => {
    setCards(await listCards())
  }, [])

  // Cards come and go: look for them while this screen is open.
  useEffect(() => {
    void refresh()
    if (root) void libraryFolders(root).then(setFolders)
    const t = window.setInterval(() => void refresh(), 4000)
    return () => window.clearInterval(t)
  }, [root, refresh])

  const set = (card: string, b: Busy | undefined) => setState((s) => ({ ...s, [card]: b }))

  async function preview(card: string) {
    set(card, { kind: 'planning' })
    try {
      set(card, { kind: 'plan', plan: await planCard(root, card) })
    } catch (e) {
      set(card, undefined)
      setMessage(e instanceof Error ? e.message : String(e))
    }
  }

  async function sync(card: string) {
    set(card, { kind: 'syncing', done: 0, total: 0, bytes: 0 })
    try {
      const report = await track({ id: `sync:${card}`, label: 'Syncing your card', unit: 'files', cancel: () => void cancelSync(card) }, (update) =>
        syncCard(root, card, (done, total, bytes) => {
          set(card, { kind: 'syncing', done, total, bytes })
          update(done, total)
        }),
      )
      set(card, { kind: 'done', report })
      await refresh()
    } catch (e) {
      set(card, undefined)
      setMessage(e instanceof Error ? e.message : String(e))
    }
  }

  async function eject(card: string) {
    set(card, { kind: 'ejecting' })
    try {
      await ejectCard(card)
      setMessage('Safe to remove the card.')
      set(card, undefined)
      await refresh()
    } catch (e) {
      set(card, undefined)
      setMessage(e instanceof Error ? e.message : String(e))
    }
  }

  if (!root) return <div className="grid h-full place-items-center text-ink/60">Choose a library first.</div>
  const known = cards.filter((c) => c.card)
  const blank = cards.filter((c) => !c.card)

  return (
    <div className="mx-auto max-w-[980px] pb-20">
      <header className="pt-6">
        <h1 className="text-[40px] font-semibold leading-none tracking-[-0.035em]">Devices</h1>
        <p className="mt-4 max-w-[66ch] text-[15px] leading-relaxed text-ink/70">
          Each SD card carries your library, written for the player that reads it. Only changes are written: new music
          is copied, edited tags are updated in place, and music already on a card is recognised and moved instead of
          copied again. Files this app didn’t put on a card are never touched.
        </p>
      </header>
      {message && (
        <p className="mt-4 cursor-pointer text-[13.5px] text-ink/80" onClick={() => setMessage('')}>
          {message}
        </p>
      )}

      {!inApp() && <p className="mt-8 text-[14px] text-ink/55">Cards are detected by the desktop app.</p>}
      {inApp() && cards.length === 0 && (
        <div className="glass mt-8 grid place-items-center rounded-3xl px-6 py-12 text-center">
          <div className="grid h-12 w-12 place-items-center rounded-2xl" style={{ background: 'color-mix(in oklab, var(--accent) 28%, transparent)' }}>
            <IconCards />
          </div>
          <p className="mt-3 text-[15px]">Insert an SD card</p>
          <p className="mt-1 text-[12.5px] text-ink/50">It appears here within a few seconds.</p>
        </div>
      )}

      <div className="mt-8 grid gap-3">
        {known.map((c) => (
          <CardTile
            key={c.volume.root}
            status={c}
            busy={state[c.volume.root]}
            folders={folders}
            onPreview={() => preview(c.volume.root)}
            onSync={() => sync(c.volume.root)}
            onCancel={() => void cancelSync(c.volume.root)}
            onEject={() => eject(c.volume.root)}
            onDismiss={() => set(c.volume.root, undefined)}
            onSaved={refresh}
            root={root}
          />
        ))}
      </div>

      {blank.length > 0 && (
        <section className="mt-10">
          <h2 className="mb-3 text-[15px] font-medium text-ink/80">Not set up yet</h2>
          <div className="grid gap-3">
            {blank.map((c) => (
              <SetupTile key={c.volume.root} status={c} folders={folders} onDone={refresh} />
            ))}
          </div>
        </section>
      )}
    </div>
  )
}

function CardTile(props: {
  status: CardStatus
  busy?: Busy
  folders: string[]
  onPreview: () => void
  onSync: () => void
  onCancel: () => void
  onEject: () => void
  onDismiss: () => void
  onSaved: () => void
  root: string
}) {
  const { volume: v, card, last_sync, files } = props.status
  const [editing, setEditing] = useState(false)
  const [testing, setTesting] = useState(false)
  const used = v.total_bytes - v.free_bytes
  const b = props.busy
  const incoming = b?.kind === 'plan' ? Math.max(0, b.plan.bytes_to_copy - b.plan.bytes_freed) : 0
  return (
    <motion.div layout transition={spring} className="glass overflow-hidden rounded-[22px] px-6 py-5">
      <div className="flex items-start gap-4">
        <div className="min-w-0 flex-1">
          <div className="flex items-baseline gap-3">
            <span className="text-[20px] font-semibold tracking-[-0.02em]">{card?.name}</span>
            <span className="text-[12.5px] text-ink/50">
              {v.root} {v.label && `(${v.label})`} {v.filesystem}
            </span>
          </div>
          <div className="mt-1 text-[12.5px] text-ink/55">
            {files ? `${files.toLocaleString()} files from your library` : 'Not synced yet'}
            {last_sync ? `, last synced ${new Date(last_sync * 1000).toLocaleString()}` : ''}
            {card?.include.length ? `. Carries: ${card.include.join(', ')}` : '. Carries: everything'}
            {card?.auto_sync ? '. Syncs when inserted' : ''}
            {card?.player && card.player !== 'standard' ? `. Written for ${PLAYER_NAMES[card.player]}` : ''}
          </div>
        </div>
        <button onClick={() => setEditing(!editing)} className="rounded-full px-3 py-1.5 text-[12.5px] text-ink/65 hover:bg-ink/10">
          {editing ? 'Close' : 'Settings'}
        </button>
      </div>

      {/* Storage: used, what this sync adds, free. */}
      <div className="mt-4">
        <div className="flex h-2.5 overflow-hidden rounded-full bg-ink/[0.07]">
          <div className="h-full bg-ink/35" style={{ width: `${(used / Math.max(1, v.total_bytes)) * 100}%` }} />
          <motion.div
            className="h-full"
            style={{ background: 'var(--accent)' }}
            animate={{ width: `${(incoming / Math.max(1, v.total_bytes)) * 100}%` }}
          />
        </div>
        <div className="mt-1.5 text-[12px] text-ink/50">
          {gb(used)} used, {gb(v.free_bytes)} free of {gb(v.total_bytes)}
          {incoming ? `; this sync adds ${gb(incoming)}` : ''}
        </div>
      </div>

      <AnimatePresence initial={false}>
        {editing && card && (
          <motion.div initial={{ height: 0, opacity: 0 }} animate={{ height: 'auto', opacity: 1 }} exit={{ height: 0, opacity: 0 }} className="overflow-hidden">
            <CardSettings root={v.root} name={card.name} include={card.include} autoSync={card.auto_sync} player={card.player ?? 'standard'} folders={props.folders} onSaved={() => { setEditing(false); props.onSaved() }} />
          </motion.div>
        )}
      </AnimatePresence>

      <AnimatePresence initial={false}>
        {testing && card && (
          <motion.div initial={{ height: 0, opacity: 0 }} animate={{ height: 'auto', opacity: 1 }} exit={{ height: 0, opacity: 0 }} className="overflow-hidden">
            <PlayerTest root={props.root} card={v.root} saved={card.test_results ?? {}} onClose={() => setTesting(false)} onSaved={props.onSaved} />
          </motion.div>
        )}
      </AnimatePresence>

      <div className="mt-4 flex flex-wrap items-center gap-2">
        {!b && (
          <>
            <PillButton onClick={props.onPreview} accent>
              Preview sync
            </PillButton>
            <PillButton onClick={props.onEject}>Eject</PillButton>
            {card && !testing && (
              <button onClick={() => setTesting(true)} className="rounded-full px-3 py-1.5 text-[13px] text-ink/70 hover:bg-ink/10">
                Test this player
              </button>
            )}
          </>
        )}
        {b?.kind === 'planning' && <span className="text-[13px] text-ink/65">Comparing the card with your library…</span>}
        {b?.kind === 'plan' && (
          <>
            <span className="mr-2 text-[13.5px] text-ink/85">
              {describePlan(b.plan)}
            </span>
            {b.plan.fits ? (
              <PillButton onClick={props.onSync} accent>
                Sync now
              </PillButton>
            ) : (
              <span className="text-[13px] text-problem">
                Not enough space. Choose fewer folders in Settings.
              </span>
            )}
            <button onClick={props.onDismiss} className="rounded-full px-3 py-1.5 text-[13px] text-ink/65 hover:bg-ink/10">
              Cancel
            </button>
          </>
        )}
        {b?.kind === 'syncing' && (
          <div className="flex w-full items-center gap-4">
            <div className="flex-1">
              <div className="text-[13px] text-ink/80">
                {b.total ? `Syncing: ${b.done.toLocaleString()} of ${b.total.toLocaleString()} changes, ${gb(b.bytes)} written` : 'Starting…'}
              </div>
              <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-ink/10">
                <motion.div className="h-full rounded-full" style={{ background: 'var(--accent)' }} animate={{ width: `${b.total ? (b.done / b.total) * 100 : 0}%` }} />
              </div>
            </div>
            <button onClick={props.onCancel} className="rounded-full px-3 py-1.5 text-[13px] text-ink/70 hover:bg-ink/10">
              Stop
            </button>
          </div>
        )}
        {b?.kind === 'done' && (
          <>
            <span className="mr-2 text-[13.5px] text-ink/85">
              {b.report.cancelled ? 'Stopped; run it again to finish. ' : 'Card is up to date. '}
              {describeReport(b.report)}
            </span>
            <PillButton onClick={props.onEject} accent>
              Eject
            </PillButton>
            <button onClick={props.onDismiss} className="rounded-full px-3 py-1.5 text-[13px] text-ink/65 hover:bg-ink/10">
              Done
            </button>
          </>
        )}
        {b?.kind === 'ejecting' && <span className="text-[13px] text-ink/65">Ejecting…</span>}
      </div>
    </motion.div>
  )
}

function describePlan(p: SyncPlan) {
  const parts = [
    p.copy && `copy ${p.copy.toLocaleString()} files (${gb(p.bytes_to_copy)})`,
    p.patch && `update tags on ${p.patch}`,
    p.moves && `move ${p.moves}`,
    p.adopt && `reuse ${p.adopt} already on the card`,
    p.delete && `remove ${p.delete}`,
  ].filter(Boolean)
  return parts.length ? `Will ${parts.join(', ')}.` : 'Already up to date.'
}

function describeReport(r: SyncReport) {
  const parts = [
    r.copied && `${r.copied} copied`,
    r.patched && `${r.patched} updated`,
    (r.moved || r.adopted) && `${r.moved + r.adopted} moved`,
    r.deleted && `${r.deleted} removed`,
  ].filter(Boolean)
  return `${parts.join(', ') || 'Nothing to change'} in ${(r.elapsed_ms / 1000).toFixed(0)} s.${
    r.errors.length ? ` ${r.errors.length} files failed and were left as they were.` : ''
  }`
}

function CardSettings(props: {
  root: string
  name: string
  include: string[]
  autoSync: boolean
  player?: PlayerId
  folders: string[]
  onSaved: () => void
}) {
  const [name, setName] = useState(props.name)
  const [include, setInclude] = useState<string[]>(props.include)
  const [auto, setAuto] = useState(props.autoSync)
  const [player, setPlayer] = useState<PlayerId>(props.player ?? 'poweramp')
  const [list, setList] = useState<PlayerInfo[]>([])
  useEffect(() => {
    players().then(setList).catch(() => setList([]))
  }, [])
  const chosen = list.find((p) => p.id === player)
  return (
    <div className="mt-4 grid gap-3 border-t border-ink/[0.06] pt-4">
      <label className="grid max-w-[360px] gap-1">
        <span className="text-[11.5px] text-ink/50">Card name</span>
        <input value={name} onChange={(e) => setName(e.target.value)} className="rounded-lg bg-ink/[0.06] px-3 py-1.5 text-[13.5px] outline-none focus:ring-1 focus:ring-[var(--accent)]" />
      </label>
      <div>
        <div className="text-[11.5px] text-ink/50">What this card carries (none ticked means everything)</div>
        <div className="mt-2 flex flex-wrap gap-2">
          {props.folders.map((f) => {
            const on = include.includes(f)
            return (
              <button
                key={f}
                onClick={() => setInclude(on ? include.filter((x) => x !== f) : [...include, f])}
                className="rounded-full px-3 py-1.5 text-[12.5px]"
                style={{ background: on ? 'color-mix(in oklab, var(--accent) 35%, transparent)' : 'color-mix(in oklab, var(--ink) 7%, transparent)' }}
              >
                {f}
              </button>
            )
          })}
        </div>
      </div>
      <div>
        <div className="text-[11.5px] text-ink/50">The player that reads this card</div>
        <div className="mt-2 flex flex-wrap gap-2" role="radiogroup" aria-label="Player">
          {list.map((p) => (
            <button
              key={p.id}
              role="radio"
              aria-checked={player === p.id}
              onClick={() => setPlayer(p.id)}
              className="rounded-full px-3 py-1.5 text-[12.5px]"
              style={{ background: player === p.id ? 'color-mix(in oklab, var(--accent) 35%, transparent)' : 'color-mix(in oklab, var(--ink) 7%, transparent)' }}
            >
              {p.label}
            </button>
          ))}
        </div>
        {chosen && (
          <ul className="mt-2 grid gap-0.5 text-[12.5px] text-ink/60">
            {chosen.notes.map((n) => (
              <li key={n}>· {n}</li>
            ))}
            <li>· The sound itself is never re-encoded; your library keeps its full tags.</li>
          </ul>
        )}
      </div>
      <label className="flex items-center gap-2 text-[13px] text-ink/80">
        <input type="checkbox" checked={auto} onChange={(e) => setAuto(e.target.checked)} className="accent-[var(--accent)]" />
        Sync automatically when this card is inserted
      </label>
      <div>
        <PillButton onClick={async () => { await setupCard(props.root, name || 'Card', include, auto, player); props.onSaved() }} accent>
          Save
        </PillButton>
      </div>
    </div>
  )
}

function SetupTile(props: { status: CardStatus; folders: string[]; onDone: () => void }) {
  const v = props.status.volume
  const [open, setOpen] = useState(false)
  return (
    <motion.div layout transition={spring} className="glass rounded-[22px] px-6 py-4">
      <div className="flex items-center gap-4">
        <div className="min-w-0 flex-1">
          <div className="text-[15px]">
            {v.label || 'Removable drive'} <span className="text-ink/50">{v.root}</span>
          </div>
          <div className="text-[12.5px] text-ink/50">
            {gb(v.free_bytes)} free of {gb(v.total_bytes)}, {v.filesystem}
          </div>
        </div>
        {!open && (
          <PillButton onClick={() => setOpen(true)} accent>
            Set up for music
          </PillButton>
        )}
      </div>
      <AnimatePresence>
        {open && (
          <motion.div initial={{ height: 0, opacity: 0 }} animate={{ height: 'auto', opacity: 1 }} className="overflow-hidden">
            <CardSettings root={v.root} name={v.label || 'My card'} include={[]} autoSync={false} folders={props.folders} onSaved={props.onDone} />
          </motion.div>
        )}
      </AnimatePresence>
    </motion.div>
  )
}

/** What the answers to the player test suggest, in words. */
function findings(r: Record<string, boolean>): string[] {
  const out: string[] = []
  if (r.artists_semicolon === false && r.artists_entries === true)
    out.push('It reads separate artist entries better than “;”. Choose “Exact copy” for this player.')
  if (r.cover_1400 === false && r.cover_600 === true) out.push('Large covers fail: choose Snowsky Echo Mini, which keeps covers at 600 px.')
  if (r.lyrics_file === false && r.lyrics_tag === true) out.push('It reads lyrics inside the file only; the app always writes both, so lyrics will show.')
  if (r.lyrics_tag === false && r.lyrics_file === true) out.push('It reads lyrics files only; the app always writes both, so lyrics will show.')
  if (r.lyrics_file === false && r.lyrics_bom === true) out.push('Its lyrics files need a byte-order mark. Tell me, and I will add that to its profile.')
  if (r.hindi === false) out.push('Devanagari shows as boxes: this player lacks the font. Song titles in Hindi will not be readable on it.')
  if (r.playlist_relative === false && r.playlist_top === false) out.push('It reads no playlist files: use its favourites, or choose Echo Mini (no playlists are written).')
  if (r.playlist_relative === false && r.playlist_top === true) out.push('Playlists need paths from the top of the card. Tell me, and I will add that to its profile.')
  if (r.year_original === false) out.push('It shows the date tag; the card copies put the original year there, so years will be right.')
  if (!out.length && Object.keys(r).length) out.push('Everything you checked works with the current profile.')
  return out
}

function PlayerTest(props: { root: string; card: string; saved: Record<string, boolean>; onClose: () => void; onSaved: () => void }) {
  const [items, setItems] = useState<TestItem[] | null>(null)
  const [answers, setAnswers] = useState<Record<string, boolean>>(props.saved)
  const [status, setStatus] = useState('')
  async function write() {
    setStatus('Writing the test to the card…')
    try {
      setItems(await playerTestWrite(props.root, props.card))
      setStatus('')
    } catch (e) {
      setStatus(e instanceof Error ? e.message : String(e))
    }
  }
  return (
    <div className="mt-4 rounded-2xl bg-ink/[0.04] p-4">
      <div className="text-[14px] font-medium">Test this player</div>
      <p className="mt-1 max-w-[62ch] text-[12.5px] leading-relaxed text-ink/60">
        Writes an album called “Player test” to the card: one of your songs in nine versions, and two playlists. Put the card in
        the player, let it rescan, then answer below. About 15 minutes. Nothing else on the card changes.
      </p>
      {!items && (
        <div className="mt-3 flex gap-2">
          <PillButton onClick={() => void write()} accent>
            Write the test to the card
          </PillButton>
          <button onClick={props.onClose} className="rounded-full px-3 py-1.5 text-[13px] text-ink/65 hover:bg-ink/10">
            Close
          </button>
        </div>
      )}
      {items && (
        <>
          <div className="mt-3 grid gap-1.5">
            {items.map((it) => (
              <div key={it.id} className="flex flex-wrap items-center gap-3 rounded-xl px-3 py-2 hover:bg-ink/[0.04]">
                <div className="min-w-[240px] flex-1">
                  <div className="text-[13px]">{it.checks}</div>
                  <div className="text-[11.5px] text-ink/45">{it.file}</div>
                </div>
                <div className="inline-flex rounded-full bg-ink/[0.07] p-0.5" role="radiogroup" aria-label={it.checks}>
                  {([true, false] as const).map((v) => (
                    <button
                      key={String(v)}
                      role="radio"
                      aria-checked={answers[it.id] === v}
                      onClick={() => setAnswers({ ...answers, [it.id]: v })}
                      className="rounded-full px-3 py-1 text-[12px]"
                      style={{ background: answers[it.id] === v ? 'color-mix(in oklab, var(--accent) 40%, transparent)' : 'transparent' }}
                    >
                      {v ? 'Yes' : 'No'}
                    </button>
                  ))}
                </div>
              </div>
            ))}
          </div>
          {findings(answers).length > 0 && (
            <ul className="mt-3 grid gap-1 rounded-xl px-3 py-2.5 text-[12.5px] text-ink/80" style={{ background: 'color-mix(in oklab, var(--accent) 12%, transparent)' }}>
              {findings(answers).map((f) => (
                <li key={f}>· {f}</li>
              ))}
            </ul>
          )}
          <div className="mt-3 flex flex-wrap gap-2">
            <PillButton
              onClick={async () => {
                await playerTestSave(props.card, answers)
                setStatus('Answers saved on the card.')
                props.onSaved()
              }}
              accent
            >
              Save the answers
            </PillButton>
            <button
              onClick={async () => {
                await playerTestRemove(props.card)
                setItems(null)
                setStatus('The test was removed from the card.')
              }}
              className="rounded-full px-3 py-1.5 text-[13px] text-ink/65 hover:bg-ink/10"
            >
              Remove the test from the card
            </button>
          </div>
        </>
      )}
      {status && <p className="mt-2 text-[12.5px] text-ink/60">{status}</p>}
    </div>
  )
}
