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
  reidentifyCard,
  syncCard,
  type CardConfig,
  type CardStatus,
  type SyncPlan,
  type SyncReport,
  playerTestRemove,
  playerTestSave,
  playerTestWrite,
  type PlayerId,
  type TestItem,
} from '../lib/engine'
import { CardEditor } from './cards/CardEditor'
import { PillButton } from './libraryParts'
import { IconCards } from '../components/Icons'
import { Page } from '../components/ui'
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
    <Page
      title="Devices"
      line="A card carries your whole library or a selection of it, laid out as your library is, for your player, a car, or someone else. Only changes are written, and files Venuta didn’t put on a card are never touched."
    >
      {message && (
        <p className="mt-4 cursor-pointer text-[13.5px] text-ink/80" onClick={() => setMessage('')}>
          {message}
        </p>
      )}

      {!inApp() && <p className="mt-8 text-[14px] text-ink/55">Cards are detected by the desktop app.</p>}
      {inApp() && cards.length === 0 && (
        <div className="glass grid place-items-center rounded-3xl px-6 py-12 text-center">
          <div className="grid h-12 w-12 place-items-center rounded-2xl" style={{ background: 'color-mix(in oklab, var(--accent) 28%, transparent)' }}>
            <IconCards />
          </div>
          <p className="mt-3 text-[15px] font-medium">Insert an SD card</p>
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
              <SetupTile key={c.volume.root} root={root} status={c} folders={folders} onDone={refresh} />
            ))}
          </div>
        </section>
      )}
    </Page>
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
            {card && describeCard(card)}
            <br />
            {files ? `${files.toLocaleString()} files on it from your library` : 'Not synced yet'}
            {last_sync ? `, last synced ${new Date(last_sync * 1000).toLocaleString()}` : ''}
            {card?.auto_sync ? '. Syncs when inserted.' : '.'}
          </div>
        </div>
        <button onClick={() => setEditing(!editing)} className="btn-ghost btn-sm">
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
            <CardEditor
              root={props.root}
              card={v.root}
              initial={card}
              defaultName={card.name}
              folders={props.folders}
              onSaved={() => {
                setEditing(false)
                props.onSaved()
              }}
            />
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
              <button onClick={() => setTesting(true)} className="btn-ghost">
                Test this player
              </button>
            )}
          </>
        )}
        {b?.kind === 'planning' && <span className="text-[13px] text-ink/65">Comparing the card with your library…</span>}
        {b?.kind === 'plan' && (
          <>
            {b.plan.warnings.length > 0 && (
              <ul className="mb-1 grid w-full gap-1 rounded-xl px-3 py-2.5 text-[12.5px] text-ink/85" style={{ background: 'color-mix(in oklab, var(--fix) 16%, transparent)' }}>
                {b.plan.warnings.map((w) => (
                  <li key={w} className="flex flex-wrap items-center gap-3">
                    {w}
                    {w.includes('copied from another card') && (
                      <button
                        className="btn btn-sm"
                        onClick={async () => {
                          await reidentifyCard(v.root)
                          props.onSaved()
                          props.onPreview()
                        }}
                      >
                        Make it a card of its own
                      </button>
                    )}
                  </li>
                ))}
              </ul>
            )}
            <span className="mr-2 text-[13.5px] text-ink/85">
              {b.plan.songs.toLocaleString('en-IN')} {b.plan.songs === 1 ? 'song' : 'songs'} once synced. {describePlan(b.plan)}
            </span>
            {b.plan.fits ? (
              <PillButton onClick={props.onSync} accent>
                Sync now
              </PillButton>
            ) : (
              <span className="text-[13px] text-problem">
                {gb(b.plan.bytes_to_copy + b.plan.reserve_bytes - b.plan.free_bytes - b.plan.bytes_freed)} too much for the card. Choose less in Settings, or turn on smaller files.
              </span>
            )}
            <button onClick={props.onDismiss} className="btn-ghost">
              Cancel
            </button>
          </>
        )}
        {b?.kind === 'syncing' && (
          <div className="flex w-full items-center gap-4">
            <div className="flex-1">
              <div className="text-[13px] text-ink/80">
                {b.total
                  ? `${b.bytes ? 'Writing to the card' : 'Converting'}: ${b.done.toLocaleString()} of ${b.total.toLocaleString()} steps${b.bytes ? `, ${gb(b.bytes)} written` : ''}`
                  : 'Starting…'}
              </div>
              <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-ink/10">
                <motion.div className="h-full rounded-full" style={{ background: 'var(--accent)' }} animate={{ width: `${b.total ? (b.done / b.total) * 100 : 0}%` }} />
              </div>
            </div>
            <button onClick={props.onCancel} className="btn-ghost">
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
            <button onClick={props.onDismiss} className="btn-ghost">
              Done
            </button>
          </>
        )}
        {b?.kind === 'ejecting' && <span className="text-[13px] text-ink/65">Ejecting…</span>}
      </div>
    </motion.div>
  )
}

/** What a card is and carries, in a line. */
function describeCard(c: CardConfig) {
  const p = c.pick
  const n = (k: number, one: string, many: string) => (k ? `${k} ${k === 1 ? one : many}` : '')
  const carries = p
    ? [n(p.playlists.length, 'playlist', 'playlists'), n(p.albums.length, 'album', 'albums'), n(p.artists.length, 'artist', 'artists')].filter(Boolean).join(', ')
    : c.include.length
      ? c.include.join(', ')
      : 'everything'
  const what = c.kind === 'gift' ? 'A gift' : c.kind === 'car' ? 'For a car or USB stick' : c.player && c.player !== 'standard' ? `For ${PLAYER_NAMES[c.player]}` : 'For any player'
  const smaller = c.convert?.aac ? ', as AAC' : c.convert?.hires ? ', hi-res as 16-bit' : ''
  return `${what}, carrying ${carries}${smaller}.`
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

function SetupTile(props: { root: string; status: CardStatus; folders: string[]; onDone: () => void }) {
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
            <CardEditor root={props.root} card={v.root} initial={null} defaultName={v.label || 'My card'} folders={props.folders} onSaved={props.onDone} />
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
  if (r.playlist_relative === false && r.playlist_top === false) out.push('It reads no playlist files, so none are written to this card; use its favourites instead.')
  if (r.playlist_relative === false && r.playlist_top === true) out.push('It reads playlists only from the top of the card, so this card gets them there, with paths from the top.')
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
          <button onClick={props.onClose} className="btn-ghost">
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
              className="btn-ghost"
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
