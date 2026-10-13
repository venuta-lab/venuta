import { AnimatePresence, motion, Reorder } from 'motion/react'
import { useCallback, useEffect, useState } from 'react'
import {
  appleImport,
  confirmBackup,
  inApp,
  playlistDelete,
  playlistRename,
  playlists as loadPlaylists,
  playlistSave,
  playlistTracks,
  ratePlaylist,
  setApple,
  smartSave,
  type Condition,
  type ImportReport,
  type PlaylistEntry,
  type PlaylistsView,
  type SmartRule,
} from '../lib/engine'
import { PillButton } from './LibraryScreen'
import { Page as Frame } from '../components/ui'
import { track } from '../lib/jobs'

const spring = { type: 'spring', stiffness: 360, damping: 32 } as const
type View = { kind: 'playlist'; name: string } | { kind: 'smart'; name: string } | { kind: 'wanted' }

export function PlaylistsScreen({ root }: { root: string }) {
  const [data, setData] = useState<PlaylistsView | null>(null)
  const [view, setView] = useState<View | null>(null)
  const [message, setMessage] = useState('')
  const [importing, setImporting] = useState(false)
  const [lastImport, setLastImport] = useState<ImportReport | null>(null)
  const [creating, setCreating] = useState<string | null>(null)

  const load = useCallback(async () => {
    try {
      setData(await loadPlaylists(root))
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e))
    }
  }, [root])

  useEffect(() => {
    if (root) void load()
  }, [root, load])

  async function runImport() {
    setImporting(true)
    setMessage('')
    try {
      const r = await track({ id: 'apple', label: 'Matching your Apple Music playlists' }, () => appleImport(root))
      setLastImport(r)
      await load()
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e))
    } finally {
      setImporting(false)
    }
  }

  async function createPlaylist(name: string) {
    setCreating(null)
    if (!name.trim()) return
    await playlistSave(root, name.trim(), [])
    await load()
    setView({ kind: 'playlist', name: name.trim() })
  }

  if (!root) return <div className="grid h-full place-items-center text-ink/60">Choose a library first.</div>
  if (!inApp()) return <div className="grid h-full place-items-center text-ink/60">Playlists are managed by the desktop app.</div>

  const own = data?.playlists.filter((p) => !p.smart) ?? []
  const smartNames = new Set(data?.smart.map((s) => s.name) ?? [])
  // Open on something rather than an empty pane: the first smart playlist.
  const first = data?.smart[0]?.name
  if (!view && first) setView({ kind: 'smart', name: first })

  const apple = (
        <div className="glass mt-6 rounded-2xl p-4">
          <div className="text-[13.5px] font-medium">Apple Music</div>
          {data?.apple_ready ? (
            <>
              <p className="mt-1 text-[12px] text-ink/55">
                Bring your playlists over. Songs you have become playlists; the rest go on the want-list.
              </p>
              <div className="mt-3">
                <PillButton onClick={runImport} disabled={importing} accent>
                  {importing ? 'Reading your library…' : 'Bring playlists over'}
                </PillButton>
              </div>
            </>
          ) : (
            <AppleSetup onSaved={load} />
          )}
          {lastImport && (
            <p className="mt-3 text-[12px] leading-relaxed text-ink/70">
              {lastImport.tracks_matched} of {lastImport.tracks_total} songs found in your library, in{' '}
              {lastImport.playlists.length} playlists. {lastImport.wanted.length} albums went on the want-list.
            </p>
          )}
        </div>
  )

  return (
    <Frame title="Playlists" line={`Saved as .m3u8 files in ${data?.folder || 'your library'}, so every player and card can use them.`}>
      <div className="flex items-start gap-6">
      <aside className="thin-scroll sticky top-[68px] max-h-[calc(100vh-230px)] w-[260px] shrink-0 overflow-y-auto pr-1">

        <Section title="Smart">
          {data?.smart.map((s) => (
            <NavItem key={s.name} active={view?.kind === 'smart' && view.name === s.name} onClick={() => setView({ kind: 'smart', name: s.name })}>
              <span className="text-accent-ink" aria-hidden>✦</span>
              <span className="min-w-0 flex-1 truncate">{s.name}</span>
              <span className="text-[11.5px] text-ink/40">{data.smart_counts[s.name] ?? 0}</span>
            </NavItem>
          ))}
        </Section>
        <Section title="Yours" action={<button onClick={() => setCreating('')} className="text-[12px] text-ink/60 hover:text-ink">+ New</button>}>
          {creating !== null && (
            <form onSubmit={(e) => { e.preventDefault(); void createPlaylist(creating) }} className="px-1 pb-1">
              <input autoFocus value={creating} onChange={(e) => setCreating(e.target.value)} onBlur={() => !creating.trim() && setCreating(null)} onKeyDown={(e) => e.key === 'Escape' && setCreating(null)} placeholder="Name, then Enter" className="w-full rounded-lg bg-ink/[0.06] px-3 py-1.5 text-[13px] outline-none focus:ring-1 focus:ring-[var(--accent)]" />
            </form>
          )}
          {own.filter((p) => !smartNames.has(p.name)).length === 0 && creating === null && (
            <p className="px-3 py-1.5 text-[12.5px] text-ink/45">None yet. New makes one; Apple Music brings yours over.</p>
          )}
          {own.filter((p) => !smartNames.has(p.name)).map((p) => (
            <NavItem key={p.name} active={view?.kind === 'playlist' && view.name === p.name} onClick={() => setView({ kind: 'playlist', name: p.name })}>
              <span className="min-w-0 flex-1 truncate">{p.name}</span>
              <span className="text-[11.5px] text-ink/40">{p.tracks - p.missing}</span>
            </NavItem>
          ))}
        </Section>
        <Section title="To get">
          <NavItem active={view?.kind === 'wanted'} onClick={() => setView({ kind: 'wanted' })}>
            <span className="flex-1">Want-list</span>
            <span className="text-[11.5px] text-ink/40">{data?.wanted.length ?? 0}</span>
          </NavItem>
        </Section>
        {message && <p className="mt-4 text-[12.5px] text-problem">{message}</p>}
        {apple}
      </aside>

      <section className="glass min-h-[420px] min-w-0 flex-1 rounded-[24px] p-6">
        <AnimatePresence mode="wait">
          <motion.div key={view ? JSON.stringify(view) : 'none'} initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0 }} transition={{ duration: 0.18 }}>
            {!view && <p className="py-20 text-center text-[14px] text-ink/50">Choose a playlist, or bring yours over from Apple Music.</p>}
            {view?.kind === 'playlist' && (
              <PlaylistEditor root={root} name={view.name} onChanged={load} onGone={() => { setView(null); void load() }} onRenamed={(n) => { setView({ kind: 'playlist', name: n }); void load() }} />
            )}
            {view?.kind === 'smart' && data && (
              <SmartEditor root={root} rule={data.smart.find((s) => s.name === view.name)!} onSaved={load} />
            )}
            {view?.kind === 'wanted' && <WantList wanted={data?.wanted ?? []} />}
          </motion.div>
        </AnimatePresence>
      </section>
      </div>
    </Frame>
  )
}

function Section(props: { title: string; action?: React.ReactNode; children: React.ReactNode }) {
  return (
    <div className="mt-5 first:mt-0">
      <div className="mb-1.5 flex items-center px-2 text-[12px] text-ink/45">
        <span className="flex-1">{props.title}</span>
        {props.action}
      </div>
      <div className="grid gap-0.5">{props.children}</div>
    </div>
  )
}

function NavItem(props: { active: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      onClick={props.onClick}
      className={`relative flex items-center gap-2 rounded-xl px-3 py-2 text-left text-[13.5px] ${props.active ? 'text-ink' : 'text-ink/70 hover:text-ink'}`}
    >
      {props.active && (
        <motion.span layoutId="pl-pill" transition={spring} className="absolute inset-0 rounded-xl" style={{ background: 'color-mix(in oklab, var(--accent) 24%, color-mix(in oklab, var(--ink) 5%, transparent))' }} />
      )}
      <span className="relative flex min-w-0 flex-1 items-center gap-2">{props.children}</span>
    </button>
  )
}

function AppleSetup({ onSaved }: { onSaved: () => void }) {
  const [key, setKey] = useState('')
  return (
    <div className="mt-2 grid gap-2">
      <p className="text-[12px] leading-relaxed text-ink/55">
        Your Apple Music library is read on this computer only. Enter its key once (16 characters; it is published in
        open-source Apple Music tools and not included here).
      </p>
      <input value={key} onChange={(e) => setKey(e.target.value)} placeholder="Library key" className="rounded-lg bg-ink/[0.06] px-3 py-1.5 text-[13px] outline-none focus:ring-1 focus:ring-[var(--accent)]" />
      <PillButton onClick={async () => { await setApple(null, key.trim()); onSaved() }} disabled={key.trim().length !== 16}>
        Save key
      </PillButton>
    </div>
  )
}

function PlaylistEditor(props: { root: string; name: string; onChanged: () => void; onGone: () => void; onRenamed: (n: string) => void }) {
  const [entries, setEntries] = useState<PlaylistEntry[] | null>(null)
  const [name, setName] = useState(props.name)
  const [dirty, setDirty] = useState(false)
  const [status, setStatus] = useState('')

  useEffect(() => {
    let live = true
    playlistTracks(props.root, props.name).then((e) => live && setEntries(e))
    return () => {
      live = false
    }
  }, [props.root, props.name])

  if (!entries) return <p className="text-ink/50">Loading…</p>
  const have = entries.filter((e) => !e.missing)
  const totalMin = Math.round(have.reduce((s, e) => s + e.duration_ms, 0) / 60000)

  async function save(list = entries!) {
    await playlistSave(props.root, props.name, list.filter((e) => !e.missing).map((e) => e.path))
    setDirty(false)
    setStatus('Saved.')
    props.onChanged()
  }

  async function rateAll() {
    try {
      const r = await ratePlaylist(props.root, props.name, 4)
      setStatus(`${r.changed} songs rated 4 stars.`)
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      if (msg.includes('BACKUP_NOT_CONFIRMED') && window.confirm('Rating writes to your music files (safely, and every change can be undone). Do you have a backup of this library?')) {
        await confirmBackup()
        await rateAll()
      } else setStatus(msg)
    }
  }

  return (
    <div>
      <div className="flex items-start gap-3">
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          onBlur={async () => {
            if (name.trim() && name.trim() !== props.name) {
              try {
                await playlistRename(props.root, props.name, name.trim())
                props.onRenamed(name.trim())
              } catch (e) {
                setStatus(String(e))
                setName(props.name)
              }
            }
          }}
          className="min-w-0 flex-1 bg-transparent text-[30px] font-semibold tracking-[-0.03em] outline-none"
        />
        <button onClick={rateAll} className="rounded-full px-3 py-1.5 text-[12.5px] text-ink/70 hover:bg-ink/10" title="Give every unrated song here 4 stars">
          Rate unrated ★★★★
        </button>
        <button onClick={async () => { if (window.confirm(`Delete “${props.name}”? It is moved to _Quarantine, not destroyed.`)) { await playlistDelete(props.root, props.name); props.onGone() } }} className="rounded-full px-3 py-1.5 text-[12.5px] text-ink/60 hover:bg-ink/10">
          Delete
        </button>
      </div>
      <p className="mt-1 text-[13px] text-ink/55">
        {have.length} songs, {totalMin} minutes
        {entries.length > have.length ? `, ${entries.length - have.length} not in your library` : ''}. Drag to reorder.
      </p>
      <Reorder.Group axis="y" values={entries} onReorder={(v) => { setEntries(v); setDirty(true) }} className="mt-4 grid gap-1">
        {entries.map((e, i) => (
          <Reorder.Item key={e.path || `missing-${i}-${e.title}`} value={e} className={`flex cursor-grab items-center gap-3 rounded-xl px-3 py-2 active:cursor-grabbing ${e.missing ? 'opacity-45' : 'hover:bg-ink/[0.05]'}`} whileDrag={{ scale: 1.02, boxShadow: '0 12px 30px -10px rgb(0 0 0 / 0.5)' }}>
            <span className="w-6 text-right text-[12px] tabular-nums text-ink/40">{i + 1}</span>
            <div className="min-w-0 flex-1">
              <div className="truncate text-[14px]">{e.title}</div>
              <div className="truncate text-[12px] text-ink/45">{e.missing ? 'Not in your library' : e.artist}</div>
            </div>
            <button
              onClick={() => { setEntries(entries.filter((x) => x !== e)); setDirty(true) }}
              className="rounded-full px-2 text-[16px] text-ink/40 hover:text-ink"
              aria-label="Remove from playlist"
            >
              ×
            </button>
          </Reorder.Item>
        ))}
      </Reorder.Group>
      <div className="mt-4 flex items-center gap-3">
        <span className="text-[12.5px] text-ink/55">{status}</span>
        {dirty && (
          <div className="ml-auto">
            <PillButton onClick={() => save()} accent>
              Save changes
            </PillButton>
          </div>
        )}
      </div>
    </div>
  )
}

const FIELDS: { id: string; label: string; ops: string[]; hint: string }[] = [
  { id: 'rating', label: 'Rating (stars)', ops: ['at_least', 'at_most', 'is'], hint: '4' },
  { id: 'unrated', label: 'Not rated yet', ops: [''], hint: '' },
  { id: 'album_has_rating', label: 'Album has a song rated', ops: ['at_least'], hint: '4' },
  { id: 'genre', label: 'Genre', ops: ['contains', 'is'], hint: 'Filmi' },
  { id: 'language', label: 'Language', ops: ['is'], hint: 'hin' },
  { id: 'category', label: 'Category', ops: ['is', 'contains'], hint: 'Film & Series' },
  { id: 'composer', label: 'Composer', ops: ['contains', 'is'], hint: 'A.R. Rahman' },
  { id: 'lyricist', label: 'Lyricist', ops: ['contains', 'is'], hint: 'Gulzar' },
  { id: 'album_artist', label: 'Album artist', ops: ['contains', 'is'], hint: '' },
  { id: 'year', label: 'Year', ops: ['at_least', 'at_most', 'is'], hint: '2004' },
  { id: 'added_days', label: 'Added in the last (days)', ops: ['at_most'], hint: '30' },
]
const OP_LABEL: Record<string, string> = { at_least: 'at least', at_most: 'at most', is: 'is', contains: 'contains', '': '' }

function SmartEditor(props: { root: string; rule: SmartRule; onSaved: () => void }) {
  const [rule, setRule] = useState<SmartRule>(props.rule)
  const [status, setStatus] = useState('')
  const set = (i: number, c: Partial<Condition>) =>
    setRule({ ...rule, conditions: rule.conditions.map((x, j) => (j === i ? { ...x, ...c } : x)) })
  return (
    <div>
      <h2 className="text-[30px] font-semibold tracking-[-0.03em]">{rule.name}</h2>
      <p className="mt-1 text-[13px] text-ink/55">A smart playlist rewrites itself from these rules whenever you open Playlists.</p>
      <div className="mt-5 flex items-center gap-2 text-[13.5px]">
        Songs matching
        <select value={rule.match} onChange={(e) => setRule({ ...rule, match: e.target.value as 'all' | 'any' })} className="rounded-lg bg-ink/[0.08] px-2 py-1 outline-none">
          <option value="all">all</option>
          <option value="any">any</option>
        </select>
        of:
      </div>
      <div className="mt-3 grid gap-2">
        {rule.conditions.map((c, i) => {
          const f = FIELDS.find((x) => x.id === c.field) ?? FIELDS[0]
          return (
            <motion.div layout key={i} className="flex flex-wrap items-center gap-2">
              <select value={c.field} onChange={(e) => { const nf = FIELDS.find((x) => x.id === e.target.value)!; set(i, { field: nf.id, op: nf.ops[0], value: nf.hint }) }} className="rounded-lg bg-ink/[0.08] px-2 py-1.5 text-[13px] outline-none">
                {FIELDS.map((x) => (
                  <option key={x.id} value={x.id}>{x.label}</option>
                ))}
              </select>
              {f.ops[0] !== '' && (
                <>
                  <select value={c.op} onChange={(e) => set(i, { op: e.target.value })} className="rounded-lg bg-ink/[0.08] px-2 py-1.5 text-[13px] outline-none">
                    {f.ops.map((o) => (
                      <option key={o} value={o}>{OP_LABEL[o]}</option>
                    ))}
                  </select>
                  <input value={c.value} onChange={(e) => set(i, { value: e.target.value })} placeholder={f.hint} className="w-48 rounded-lg bg-ink/[0.06] px-3 py-1.5 text-[13px] outline-none focus:ring-1 focus:ring-[var(--accent)]" />
                </>
              )}
              <button onClick={() => setRule({ ...rule, conditions: rule.conditions.filter((_, j) => j !== i) })} className="px-2 text-ink/40 hover:text-ink" aria-label="Remove rule">
                ×
              </button>
            </motion.div>
          )
        })}
      </div>
      <button onClick={() => setRule({ ...rule, conditions: [...rule.conditions, { field: 'rating', op: 'at_least', value: '4' }] })} className="mt-3 rounded-full px-3 py-1.5 text-[12.5px] text-ink/70 hover:bg-ink/10">
        + Add a rule
      </button>
      <label className="mt-4 flex items-center gap-2 text-[13px] text-ink/75">
        At most
        <input type="number" min={0} value={rule.limit ?? ''} onChange={(e) => setRule({ ...rule, limit: e.target.value ? Number(e.target.value) : null })} className="w-24 rounded-lg bg-ink/[0.06] px-3 py-1.5 outline-none" />
        songs (empty for no limit)
      </label>
      <div className="mt-5 flex items-center gap-3">
        <PillButton onClick={async () => { await smartSave(props.root, rule); setStatus('Saved. The playlist was rewritten.'); props.onSaved() }} accent>
          Save rule
        </PillButton>
        <span className="text-[12.5px] text-ink/55">{status}</span>
      </div>
    </div>
  )
}

function WantList({ wanted }: { wanted: PlaylistsView['wanted'] }) {
  const [tier, setTier] = useState<1 | 2 | 3>(1)
  const shown = wanted.filter((w) => w.tier === tier)
  const label = { 1: 'Get the whole album', 2: 'Explore', 3: 'Probably skip' } as const
  if (!wanted.length) return <p className="py-20 text-center text-[14px] text-ink/50">Bring your Apple Music playlists over to build the want-list.</p>
  return (
    <div>
      <h2 className="text-[30px] font-semibold tracking-[-0.03em]">Want-list</h2>
      <p className="mt-1 max-w-[62ch] text-[13px] leading-relaxed text-ink/55">
        Albums from your playlists that aren’t in your library yet. A spreadsheet copy is saved as wanted-albums.csv next to your
        rules. This list never downloads anything.
      </p>
      <div className="glass mt-5 inline-flex rounded-full p-1">
        {([1, 2, 3] as const).map((t) => (
          <button key={t} onClick={() => setTier(t)} className={`relative rounded-full px-4 py-1.5 text-[13px] ${tier === t ? 'text-ink' : 'text-ink/60'}`}>
            {tier === t && <motion.span layoutId="tier-pill" transition={spring} className="absolute inset-0 rounded-full" style={{ background: 'color-mix(in oklab, var(--accent) 28%, color-mix(in oklab, var(--ink) 6%, transparent))' }} />}
            <span className="relative">
              {label[t]} <span className="text-ink/45">{wanted.filter((w) => w.tier === t).length}</span>
            </span>
          </button>
        ))}
      </div>
      <div className="mt-4 grid gap-1">
        {shown.map((w) => (
          <div key={w.album + w.artist} className="rounded-xl px-3 py-2.5 hover:bg-ink/[0.05]">
            <div className="flex items-baseline gap-3">
              <span className="min-w-0 flex-1 truncate text-[14px]">{w.album}</span>
              <span className="shrink-0 text-[12px] text-ink/45">{w.saved_tracks} saved</span>
            </div>
            <div className="truncate text-[12px] text-ink/50">
              {w.artist}. {w.why}
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}

/** "Add to playlist" for a set of library tracks: appends them (skipping ones already there). */
export function AddToPlaylist({ root, paths }: { root: string; paths: string[] }) {
  const [open, setOpen] = useState(false)
  const [names, setNames] = useState<string[]>([])
  const [fresh, setFresh] = useState('')
  const [done, setDone] = useState('')

  useEffect(() => {
    if (!open) return
    loadPlaylists(root).then((v) => setNames(v.playlists.filter((p) => !p.smart).map((p) => p.name)))
  }, [open, root])

  async function add(name: string) {
    const existing = names.includes(name) ? await playlistTracks(root, name) : []
    const have = existing.filter((e) => !e.missing).map((e) => e.path)
    const added = paths.filter((p) => !have.includes(p))
    await playlistSave(root, name, [...have, ...added])
    setDone(`Added ${added.length} to ${name}`)
    setOpen(false)
    setFresh('')
    window.setTimeout(() => setDone(''), 2500)
  }

  if (!inApp() || !paths.length) return null
  return (
    <div className="relative">
      <button onClick={() => setOpen(!open)} className="rounded-full px-4 py-2 text-[13px] text-ink/70 hover:bg-ink/10">
        {done || 'Add to playlist'}
      </button>
      <AnimatePresence>
        {open && (
          <motion.div
            initial={{ opacity: 0, y: 6, scale: 0.97 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 6, scale: 0.97 }}
            transition={spring}
            className="popover absolute bottom-full right-0 z-10 mb-2 w-64 rounded-2xl p-2"
            style={{ background: 'rgb(26 26 32 / 0.94)' }}
          >
            <div className="no-scrollbar max-h-60 overflow-y-auto">
              {names.map((n) => (
                <button key={n} onClick={() => add(n)} className="block w-full truncate rounded-lg px-3 py-1.5 text-left text-[13px] hover:bg-ink/10">
                  {n}
                </button>
              ))}
            </div>
            <form
              onSubmit={(e) => {
                e.preventDefault()
                if (fresh.trim()) void add(fresh.trim())
              }}
              className="mt-1 border-t border-ink/[0.06] pt-2"
            >
              <input value={fresh} onChange={(e) => setFresh(e.target.value)} placeholder="New playlist name" className="w-full rounded-lg bg-ink/[0.06] px-3 py-1.5 text-[13px] outline-none focus:ring-1 focus:ring-[var(--accent)]" />
            </form>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  )
}
