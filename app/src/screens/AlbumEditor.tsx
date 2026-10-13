import { useEscape } from '../lib/layers'
import { AnimatePresence, motion, useDragControls } from 'motion/react'
import { useEffect, useMemo, useState } from 'react'
import {
  albumDetail,
  confirmBackup,
  editTags,
  inApp,
  type AlbumDetail,
  type AlbumSummary,
  type TagEdit,
} from '../lib/engine'
import { askAddToPlaylist } from '../components/AddToPlaylist'
import { confirmAsk } from '../components/Confirm'
import { IconClose } from '../components/Icons'

const spring = { type: 'spring', stiffness: 300, damping: 30 } as const

type AlbumField = 'album' | 'album_artist' | 'original_year' | 'genre' | 'composer' | 'lyricist'
const FIELDS: { key: AlbumField; label: string; tags: string[] }[] = [
  { key: 'album', label: 'Album', tags: ['ALBUM'] },
  { key: 'album_artist', label: 'Album artist', tags: ['ALBUMARTIST'] },
  { key: 'original_year', label: 'Original year', tags: ['ORIGINALYEAR', 'ORIGINALDATE'] },
  { key: 'genre', label: 'Genre', tags: ['GENRE'] },
  { key: 'composer', label: 'Composer', tags: ['COMPOSER'] },
  { key: 'lyricist', label: 'Lyricist', tags: ['LYRICIST'] },
]

/** Split "A; B" into tag values. */
const values = (s: string) =>
  s
    .split(';')
    .map((v) => v.trim())
    .filter(Boolean)

/** Edits change files: the first one ever waits for "I have a backup". */
async function backupFirst(): Promise<boolean> {
  const ok = await confirmAsk({
    title: 'Before your first edit',
    text: 'Edits change your music files. Every change is checked so the audio is never touched, and each one can be undone. Even so, keep a backup of this library somewhere else, like another drive or an SD card.',
    confirm: 'I have a backup',
  })
  if (ok) await confirmBackup()
  return ok
}

/**
 * Album view that doubles as an editor. Album fields and titles are edited inline and saved
 * together; star ratings save the moment they are clicked.
 */
export function AlbumEditor(props: { root: string; album: AlbumSummary; src?: string; onClose: () => void }) {
  const a = props.album
  const drag = useDragControls()
  const [detail, setDetail] = useState<AlbumDetail | null>(null)
  const [fields, setFields] = useState<Partial<Record<AlbumField, string>>>({})
  const [titles, setTitles] = useState<Record<string, string>>({})
  const [stars, setStars] = useState<Record<string, number>>({})
  const [status, setStatus] = useState('')
  const [problem, setProblem] = useState(false)
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    let live = true
    albumDetail(props.root, a.dir).then(
      (d) => live && setDetail(d),
      (e) => {
        if (!live) return
        setStatus(e instanceof Error ? e.message : String(e))
        setProblem(true)
      },
    )
    return () => {
      live = false
    }
  }, [props.root, a.dir])

  const dirty = Object.keys(fields).length + Object.keys(titles).length

  /** Close, but never lose typed changes without asking. */
  async function close() {
    if (dirty) {
      const ok = await confirmAsk({
        title: `Discard ${dirty === 1 ? 'your change' : `${dirty} changes`}?`,
        text: "They haven't been saved to the files yet.",
        confirm: 'Discard',
        danger: true,
      })
      if (!ok) return
    }
    props.onClose()
  }
  useEscape(true, () => void close())

  const edits = useMemo<TagEdit[]>(() => {
    if (!detail) return []
    const albumSets: [string, string[]][] = []
    for (const f of FIELDS) {
      const v = fields[f.key]
      if (v === undefined) continue
      for (const tag of f.tags) albumSets.push([tag, values(v)])
    }
    return detail.tracks
      .map((t) => {
        const set: [string, string[]][] = [...albumSets]
        if (titles[t.path] !== undefined) set.push(['TITLE', [titles[t.path].trim()]])
        return { file: t.path, set }
      })
      .filter((e) => e.set.length)
  }, [detail, fields, titles])

  /** Run an edit; on the first ever edit, ask for the backup confirmation and retry. */
  async function run(edit: TagEdit[]): Promise<boolean> {
    try {
      const r = await editTags(props.root, edit)
      if (r.errors.length) {
        setStatus(`${r.errors.length === 1 ? 'A file' : `${r.errors.length} files`} could not be saved: ${r.errors[0][1]}`)
        setProblem(true)
        return false
      }
      setProblem(false)
      return true
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      if (msg.includes('BACKUP_NOT_CONFIRMED')) return (await backupFirst()) ? run(edit) : false
      setStatus(msg)
      setProblem(true)
      return false
    }
  }

  async function save() {
    setSaving(true)
    setStatus('Saving…')
    setProblem(false)
    const ok = await run(edits)
    setSaving(false)
    if (!ok) return
    setFields({})
    setTitles({})
    setDetail(await albumDetail(props.root, a.dir))
    setStatus('Saved. Every change can be undone in Activity.')
  }

  async function rate(path: string, value: number) {
    const before = stars[path] ?? detail?.tracks.find((t) => t.path === path)?.stars ?? 0
    const next = before === value ? 0 : value // clicking the same star clears it
    setStars((s) => ({ ...s, [path]: next }))
    if (!(await run([{ file: path, set: [['RATING', next ? [String(next * 20)] : []]] }]))) setStars((s) => ({ ...s, [path]: before }))
  }

  const value = (k: AlbumField) => fields[k] ?? detail?.[k] ?? ''
  const totalMin = detail ? Math.round(detail.tracks.reduce((s, t) => s + t.duration_ms, 0) / 60000) : 0
  const discs = new Set(detail?.tracks.map((t) => t.disc || 1)).size

  return (
    <motion.div
      className="fixed inset-0 z-40 grid place-items-center p-8"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onClick={() => void close()}
    >
      <div className="absolute inset-0 bg-[var(--scrim)] backdrop-blur-md" />
      <motion.div
        role="dialog"
        aria-label={`${value('album') || a.title}, details`}
        onClick={(e) => e.stopPropagation()}
        className="popover relative flex max-h-full w-full max-w-[1120px] flex-col overflow-hidden rounded-[28px]"
        initial={{ y: 30, opacity: 0 }}
        animate={{ y: 0, opacity: 1 }}
        exit={{ y: 20, opacity: 0 }}
        transition={spring}
        drag="y"
        dragControls={drag}
        dragListener={false}
        dragConstraints={{ top: 0, bottom: 0 }}
        dragElastic={0.4}
        onDragEnd={(_, info) => {
          if (Math.abs(info.offset.y) > 120) void close()
        }}
      >
        {/* Grab handle: swipe down here to close. */}
        <div onPointerDown={(e) => drag.start(e)} className="flex cursor-grab justify-center pb-1 pt-3 active:cursor-grabbing" aria-hidden>
          <span className="h-1.5 w-12 rounded-full bg-ink/25" />
        </div>
        <button
          onClick={() => void close()}
          aria-label="Close (Esc)"
          title="Close (Esc)"
          className="absolute right-4 top-4 z-10 grid h-9 w-9 place-items-center rounded-full text-ink/65 transition-colors hover:bg-ink/10 hover:text-ink active:scale-90"
        >
          <IconClose />
        </button>
        <div className="flex min-h-0 flex-1 gap-8 px-8 pb-6">
          <div className="thin-scroll -mr-2 w-[300px] shrink-0 overflow-y-auto pb-2 pr-2">
            <motion.div layoutId={`cover-${a.dir}`} className="aspect-square overflow-hidden rounded-2xl bg-ink/[0.06] shadow-2xl shadow-black/50" transition={spring}>
              {props.src && <img src={props.src} alt="" draggable={false} className="h-full w-full object-cover" />}
            </motion.div>
            <div className="mt-5 grid gap-3">
              {FIELDS.map((f) => (
                <label key={f.key} className="grid gap-1">
                  <span className="text-[12px] text-ink/55">{f.label}</span>
                  <input
                    value={value(f.key)}
                    disabled={!inApp() || !detail}
                    placeholder={detail && !detail[f.key] ? 'Not set, or differs between tracks' : ''}
                    onChange={(e) => setFields((s) => ({ ...s, [f.key]: e.target.value }))}
                    className={`field w-full ${fields[f.key] !== undefined ? 'ring-1 ring-[color-mix(in_oklab,var(--accent)_60%,transparent)]' : ''}`}
                  />
                </label>
              ))}
              <p className="text-[12px] leading-relaxed text-ink/45">
                Separate several names with “;”. {detail?.tracks.length ?? a.tracks} tracks, {totalMin} minutes.
              </p>
            </div>
          </div>

          <div className="flex min-w-0 flex-1 flex-col">
            <h2 className="title pr-12 text-[32px] leading-tight">{value('album') || a.title}</h2>
            <p className="mt-1 text-[16px] text-ink/70">{value('album_artist') || a.album_artist}</p>
            <div className="thin-scroll mt-5 min-h-0 flex-1 overflow-y-auto pr-1">
              {!detail && !problem && (
                <div className="grid gap-2" aria-hidden>
                  {Array.from({ length: 8 }, (_, i) => (
                    <div key={i} className="shimmer h-11 rounded-xl" />
                  ))}
                </div>
              )}
              {detail?.tracks.map((t, i) => (
                <div key={t.path}>
                  {discs > 1 && (i === 0 || (t.disc || 1) !== (detail.tracks[i - 1].disc || 1)) ? (
                    <div className="px-2 pb-1 pt-3 text-[12px] font-medium text-ink/50">Disc {t.disc || 1}</div>
                  ) : null}
                  <div className="group grid grid-cols-[36px_1fr_auto_auto] items-center gap-3 rounded-xl px-2 py-1.5 transition-colors hover:bg-ink/[0.04]">
                    <span className="text-right text-[13px] tabular-nums text-ink/45">{t.track || ''}</span>
                    <div className="min-w-0">
                      <input
                        value={titles[t.path] ?? t.title}
                        disabled={!inApp()}
                        aria-label={`Title of track ${t.track || i + 1}`}
                        title="Click to edit the title"
                        onChange={(e) => setTitles((s) => ({ ...s, [t.path]: e.target.value }))}
                        className={`-mx-1 w-full cursor-text truncate rounded-md bg-transparent px-1 py-0.5 text-[14px] outline-none transition-colors hover:bg-ink/[0.06] focus:bg-ink/[0.08] focus:ring-1 focus:ring-[color-mix(in_oklab,var(--accent)_60%,transparent)] ${
                          titles[t.path] !== undefined ? 'text-accent-ink' : ''
                        }`}
                      />
                      {t.artist && <div className="truncate text-[12px] text-ink/50">{t.artist}</div>}
                    </div>
                    <Stars value={stars[t.path] ?? t.stars} onRate={(n) => void rate(t.path, n)} disabled={!inApp()} />
                    <span className="w-12 text-right text-[12px] tabular-nums text-ink/50">{t.duration_ms ? fmtDuration(t.duration_ms) : ''}</span>
                  </div>
                </div>
              ))}
            </div>
            <div className="mt-4 flex items-center gap-3 border-t border-edge pt-4">
              <span role="status" className={`min-w-0 flex-1 truncate text-[12.5px] ${problem ? 'text-problem' : 'text-ink/55'}`} title={status}>
                {status || (inApp() ? 'Click a field or a title to change it. Ratings save as soon as you click a star.' : 'Editing works in the desktop app.')}
              </span>
              {!dirty && inApp() && !!detail?.tracks.length && (
                <button onClick={() => askAddToPlaylist(detail.tracks.map((t) => t.path), detail.album || a.title)} className="btn-ghost btn-sm">
                  Add to playlist…
                </button>
              )}
              <AnimatePresence>
                {dirty > 0 && (
                  <motion.div initial={{ opacity: 0, x: 10 }} animate={{ opacity: 1, x: 0 }} exit={{ opacity: 0 }} className="flex gap-2">
                    <button
                      onClick={() => {
                        setFields({})
                        setTitles({})
                        setStatus('')
                      }}
                      disabled={saving}
                      className="btn"
                    >
                      Discard
                    </button>
                    <button onClick={() => void save()} disabled={saving} className="btn-primary">
                      {saving ? 'Saving…' : `Save ${dirty} ${dirty === 1 ? 'change' : 'changes'}`}
                    </button>
                  </motion.div>
                )}
              </AnimatePresence>
            </div>
          </div>
        </div>
      </motion.div>
    </motion.div>
  )
}

function Stars(props: { value: number; onRate: (n: number) => void; disabled?: boolean }) {
  const [hover, setHover] = useState(0)
  const shown = hover || props.value
  return (
    <div
      className={`flex transition-opacity ${props.value || hover ? '' : 'opacity-40 group-hover:opacity-100'}`}
      onMouseLeave={() => setHover(0)}
      role="group"
      aria-label={props.value ? `Rated ${props.value} of 5` : 'Not rated'}
    >
      {[1, 2, 3, 4, 5].map((n) => (
        <motion.button
          key={n}
          disabled={props.disabled}
          whileTap={{ scale: 1.35 }}
          onMouseEnter={() => setHover(n)}
          onFocus={() => setHover(n)}
          onBlur={() => setHover(0)}
          onClick={() => props.onRate(n)}
          className="px-[1px] text-[15px] leading-none"
          style={{ color: n <= shown ? 'var(--accent)' : 'color-mix(in oklab, var(--ink) 22%, transparent)' }}
          aria-label={n === props.value ? `${n} stars (click again to clear)` : `${n} ${n === 1 ? 'star' : 'stars'}`}
          title={n === props.value ? 'Click again to clear the rating' : undefined}
        >
          ★
        </motion.button>
      ))}
    </div>
  )
}

const fmtDuration = (ms: number) => {
  const s = Math.round(ms / 1000)
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`
}
