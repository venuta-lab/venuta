import { AnimatePresence, motion } from 'motion/react'
import { useCallback, useEffect, useRef, useState } from 'react'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import {
  addToIncoming,
  cancelApply,
  cancelLookup,
  inApp,
  incoming,
  incomingCount,
  incomingImport,
  incomingLookup,
  type ApplyReport,
  type Decision,
  type IncomingView,
} from '../lib/engine'
import { loadCovers } from '../lib/covers'
import { AlbumRow } from './ReviewScreen'
import { DeepProgressBar, PillButton } from './LibraryScreen'
import { IconIncoming } from '../components/Icons'

/** Incoming covers get their own thumbnail size so they never collide with library covers. */
const COVER = 300

export function IncomingScreen({ root, onImported }: { root: string; onImported: () => void }) {
  const [view, setView] = useState<IncomingView | null>(null)
  const [selected, setSelected] = useState<Record<string, boolean>>({})
  const [open, setOpen] = useState<string | null>(null)
  const [dragging, setDragging] = useState(false)
  const [lookup, setLookup] = useState<{ done: number; total: number } | null>(null)
  const [importing, setImporting] = useState<{ done: number; total: number } | null>(null)
  const [message, setMessage] = useState('')
  const lastCount = useRef(-1)

  const load = useCallback(async () => {
    try {
      const v = await incoming(root)
      setView(v)
      await loadCovers(v.folder, v.plan.albums.map((a) => a.id), COVER)
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e))
    }
  }, [root])

  // Plan on open, then re-plan whenever the number of waiting files changes.
  useEffect(() => {
    if (!root) return
    void load()
    const timer = window.setInterval(async () => {
      const n = await incomingCount(root)
      if (lastCount.current !== -1 && n !== lastCount.current) void load()
      lastCount.current = n
    }, 8000)
    return () => window.clearInterval(timer)
  }, [root, load])

  // Drop files or folders anywhere on the window while this screen is open.
  useEffect(() => {
    if (!inApp() || !root) return
    let unlisten: (() => void) | undefined
    getCurrentWebview()
      .onDragDropEvent(async (e) => {
        if (e.payload.type === 'over' || e.payload.type === 'enter') setDragging(true)
        else if (e.payload.type === 'leave') setDragging(false)
        else if (e.payload.type === 'drop') {
          setDragging(false)
          setMessage('Copying into Incoming…')
          try {
            const n = await addToIncoming(root, e.payload.paths)
            setMessage(`${n} files copied into Incoming. Your originals stay where they were.`)
            await load()
          } catch (err) {
            setMessage(err instanceof Error ? err.message : String(err))
          }
        }
      })
      .then((u) => (unlisten = u))
    return () => unlisten?.()
  }, [root, load])

  async function runLookup() {
    setLookup({ done: 0, total: 0 })
    try {
      const r = await incomingLookup(root, (done, total) => setLookup({ done, total }))
      setMessage(
        `Looked up ${r.albums_looked_up} albums; found lyrics for ${r.lyrics_found} of ${r.lyrics_checked} tracks.`,
      )
      await load()
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e))
    } finally {
      setLookup(null)
    }
  }

  async function runImport() {
    const ids = Object.entries(selected)
      .filter(([, on]) => on)
      .map(([id]) => id)
    setImporting({ done: 0, total: 0 })
    try {
      const r: ApplyReport = await incomingImport(root, ids, (done, total) => setImporting({ done, total }))
      setMessage(
        `Imported ${r.tracks_written} tracks into your library${r.errors.length ? `; ${r.errors.length} files could not be written and stayed in Incoming` : ''}. The originals were moved to Incoming/.imported.`,
      )
      setSelected({})
      onImported()
      await load()
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e))
    } finally {
      setImporting(null)
    }
  }

  if (!root) return <div className="grid h-full place-items-center text-ink/60">Choose a library first.</div>

  const albums = view?.plan.albums ?? []
  const count = Object.values(selected).filter(Boolean).length
  const ready = albums.filter((a) => a.action.kind === 'migrate')

  return (
    <div className="mx-auto max-w-[980px] pb-20">
      <header className="pt-6">
        <h1 className="text-[44px] font-semibold leading-none tracking-[-0.035em]">Incoming</h1>
        <p className="mt-4 max-w-[66ch] text-[15px] leading-relaxed text-ink/70">
          Drop albums anywhere on this window. They are copied into {view?.folder ?? 'Incoming'} and planned with your
          rules. Nothing reaches your library until you import it.
        </p>
      </header>

      <motion.div
        animate={{ scale: dragging ? 1.015 : 1 }}
        transition={{ type: 'spring', stiffness: 300, damping: 24 }}
        className="glass mt-6 grid place-items-center rounded-3xl border-dashed px-6 py-10 text-center"
        style={{
          borderStyle: 'dashed',
          borderWidth: 1.5,
          borderColor: dragging ? 'var(--accent)' : 'color-mix(in oklab, var(--ink) 22%, transparent)',
          background: dragging ? 'color-mix(in oklab, var(--accent) 16%, color-mix(in oklab, var(--ink) 4%, transparent))' : undefined,
        }}
      >
        <div
          className="grid h-12 w-12 place-items-center rounded-2xl"
          style={{ background: 'color-mix(in oklab, var(--accent) 28%, transparent)' }}
        >
          <IconIncoming />
        </div>
        <div className="mt-3 text-[15px]">{dragging ? 'Let go to add these' : 'Drop album folders or files here'}</div>
        <div className="mt-1 text-[12.5px] text-ink/50">
          {albums.length ? `${albums.length} albums waiting` : 'Nothing waiting yet'}
        </div>
      </motion.div>

      {albums.length > 0 && (
        <div className="mt-6 flex flex-wrap items-center gap-3">
          <PillButton onClick={runLookup} disabled={!!lookup || !!importing}>
            Look up online
          </PillButton>
          <button
            onClick={() => setSelected(Object.fromEntries(ready.map((a) => [a.id, true])))}
            className="rounded-full px-4 py-2 text-[13px] text-ink/75 hover:bg-ink/10"
          >
            Select all {ready.length}
          </button>
          <div className="ml-auto">
            <PillButton onClick={runImport} disabled={!count || !!importing || !!lookup} accent>
              {count ? `Import ${count} ${count === 1 ? 'album' : 'albums'}` : 'Choose albums to import'}
            </PillButton>
          </div>
        </div>
      )}

      <AnimatePresence>
        {lookup && <DeepProgressBar deep={lookup} label="Looking up" unit="steps" onCancel={() => void cancelLookup()} />}
      </AnimatePresence>
      <AnimatePresence>
        {importing && <DeepProgressBar deep={importing} label="Importing" unit="tracks" onCancel={() => void cancelApply()} />}
      </AnimatePresence>
      {message && <p className="mt-3 text-[13px] text-ink/70">{message}</p>}

      <div className="mt-4 flex flex-col gap-2">
        {albums.map((a) => (
          <AlbumRow
            key={a.id}
            album={a}
            coverSize={COVER}
            approveLabel="Import"
            decision={selected[a.id] ? ('approved' as Decision) : undefined}
            open={open === a.id}
            onToggle={() => setOpen(open === a.id ? null : a.id)}
            onDecide={(d) => setSelected((s) => ({ ...s, [a.id]: d === 'approved' }))}
          />
        ))}
      </div>
    </div>
  )
}
