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
  const [askBackup, setAskBackup] = useState<null | (() => Promise<void>)>(null)

  useEffect(() => {
    let live = true
    albumDetail(props.root, a.dir).then(
      (d) => live && setDetail(d),
      (e) => live && setStatus(e instanceof Error ? e.message : String(e)),
    )
    return () => {
      live = false
    }
  }, [props.root, a.dir])

  useEscape(true, props.onClose)

  const dirty = Object.keys(fields).length + Object.keys(titles).length
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
  async function run(edit: TagEdit[], after: () => void) {
    try {
      const r = await editTags(props.root, edit)
      setStatus(r.errors.length ? `${r.errors.length} files could not be saved: ${r.errors[0][1]}` : '')
      after()
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      if (msg.includes('BACKUP_NOT_CONFIRMED')) {
        setAskBackup(() => async () => {
          await confirmBackup()
          setAskBackup(null)
          await run(edit, after)
        })
      } else {
        setStatus(msg)
      }
    }
  }

  async function save() {
    setStatus('Saving…')
    await run(edits, async () => {
      setFields({})
      setTitles({})
      setDetail(await albumDetail(props.root, a.dir))
      setStatus('Saved. Every change can be undone from its journal.')
    })
  }

  async function rate(path: string, value: number) {
    const current = stars[path] ?? detail?.tracks.find((t) => t.path === path)?.stars ?? 0
    const next = current === value ? 0 : value // clicking the same star clears it
    setStars((s) => ({ ...s, [path]: next }))
    await run([{ file: path, set: [['RATING', next ? [String(next * 20)] : []]] }], () => {})
  }

  const value = (k: AlbumField) => fields[k] ?? detail?.[k] ?? ''
  const totalMin = detail ? Math.round(detail.tracks.reduce((s, t) => s + t.duration_ms, 0) / 60000) : 0

  return (
    <motion.div
      className="fixed inset-0 z-40 grid place-items-center p-8"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onClick={props.onClose}
    >
      <div className="absolute inset-0 bg-black/45 backdrop-blur-md" />
      <motion.div
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
          if (Math.abs(info.offset.y) > 120) props.onClose()
        }}
      >
        {/* Grab handle: swipe down here to close. */}
        <div
          onPointerDown={(e) => drag.start(e)}
          className="flex cursor-grab justify-center pb-1 pt-3 active:cursor-grabbing"
          aria-hidden
        >
          <span className="h-1.5 w-12 rounded-full bg-ink/25" />
        </div>
        <div className="flex min-h-0 gap-8 px-8 pb-8">
          <div className="w-[300px] shrink-0">
            <motion.div
              layoutId={`cover-${a.dir}`}
              className="aspect-square overflow-hidden rounded-2xl shadow-2xl shadow-black/50"
              transition={spring}
            >
              {props.src && <img src={props.src} alt="" draggable={false} className="h-full w-full object-cover" />}
            </motion.div>
            <div className="mt-5 grid gap-3">
              {FIELDS.map((f) => (
                <label key={f.key} className="grid gap-1">
                  <span className="text-[11.5px] text-ink/50">{f.label}</span>
                  <input
                    value={value(f.key)}
                    disabled={!inApp()}
                    placeholder={detail && !detail[f.key] ? 'Not set, or differs between tracks' : ''}
                    onChange={(e) => setFields((s) => ({ ...s, [f.key]: e.target.value }))}
                    className={`rounded-lg bg-ink/[0.06] px-3 py-1.5 text-[13.5px] outline-none ring-1 ring-transparent placeholder:text-ink/30 focus:ring-[var(--accent)] ${
                      fields[f.key] !== undefined ? 'ring-[color-mix(in_oklab,var(--accent)_60%,transparent)]' : ''
                    }`}
                  />
                </label>
              ))}
              <p className="text-[11.5px] leading-relaxed text-ink/40">
                Separate several names with “;”. {detail?.tracks.length ?? a.tracks} tracks, {totalMin} minutes.
              </p>
            </div>
          </div>

          <div className="flex min-w-0 flex-1 flex-col">
            <h2 className="text-[32px] font-semibold leading-tight tracking-[-0.03em]">{value('album') || a.title}</h2>
            <p className="mt-1 text-[16px] text-ink/70">{value('album_artist') || a.album_artist}</p>
            <div className="no-scrollbar mt-5 min-h-0 flex-1 overflow-y-auto pr-1">
              {detail?.tracks.map((t) => (
                <div
                  key={t.path}
                  className="group grid grid-cols-[36px_1fr_auto_auto] items-center gap-3 rounded-xl px-2 py-1.5 hover:bg-ink/[0.05]"
                >
                  <span className="text-right text-[13px] tabular-nums text-ink/45">
                    {t.disc > 1 ? `${t.disc}-` : ''}
                    {t.track || ''}
                  </span>
                  <div className="min-w-0">
                    <input
                      value={titles[t.path] ?? t.title}
                      disabled={!inApp()}
                      onChange={(e) => setTitles((s) => ({ ...s, [t.path]: e.target.value }))}
                      className={`w-full truncate rounded-md bg-transparent px-1 text-[14px] outline-none focus:bg-ink/[0.06] ${
                        titles[t.path] !== undefined ? 'text-accent-ink' : ''
                      }`}
                    />
                    {t.artist && <div className="truncate px-1 text-[12px] text-ink/45">{t.artist}</div>}
                  </div>
                  <Stars value={stars[t.path] ?? t.stars} onRate={(n) => rate(t.path, n)} disabled={!inApp()} />
                  <span className="w-12 text-right text-[12px] tabular-nums text-ink/45">
                    {t.duration_ms ? fmtDuration(t.duration_ms) : ''}
                  </span>
                </div>
              ))}
            </div>
            <div className="mt-4 flex items-center gap-3 border-t border-ink/[0.06] pt-4">
              <span className="min-w-0 flex-1 truncate text-[12.5px] text-ink/55">
                {status || (inApp() ? 'Edit any field or title. Ratings save as soon as you click.' : 'Editing works in the desktop app.')}
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
                      }}
                      className="rounded-full px-4 py-2 text-[13px] text-ink/70 hover:bg-ink/10"
                    >
                      Discard
                    </button>
                    <motion.button
                      whileTap={{ scale: 0.96 }}
                      onClick={save}
                      className="rounded-full px-5 py-2 text-[13.5px] font-medium text-on-accent"
                      style={{ background: 'var(--accent)' }}
                    >
                      Save {dirty} {dirty === 1 ? 'change' : 'changes'}
                    </motion.button>
                  </motion.div>
                )}
              </AnimatePresence>
            </div>
          </div>
        </div>

        <AnimatePresence>
          {askBackup && (
            <motion.div
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="absolute inset-0 grid place-items-center bg-black/55 p-8 backdrop-blur-sm"
            >
              <motion.div initial={{ scale: 0.96 }} animate={{ scale: 1 }} className="popover max-w-[460px] rounded-3xl p-7">
                <h3 className="text-[20px] font-semibold">Before your first edit</h3>
                <p className="mt-3 text-[14px] leading-relaxed text-ink/70">
                  Edits change your music files. Every change is checked so the audio is never touched, and each one
                  can be undone. Even so, keep a backup of this library somewhere else (another drive or SD card).
                </p>
                <div className="mt-6 flex justify-end gap-2">
                  <button onClick={() => setAskBackup(null)} className="rounded-full px-4 py-2 text-[13px] text-ink/70 hover:bg-ink/10">
                    Not yet
                  </button>
                  <button
                    onClick={() => void askBackup()}
                    className="rounded-full px-5 py-2 text-[13.5px] font-medium text-on-accent"
                    style={{ background: 'var(--accent)' }}
                  >
                    I have a backup
                  </button>
                </div>
              </motion.div>
            </motion.div>
          )}
        </AnimatePresence>
      </motion.div>
    </motion.div>
  )
}

function Stars(props: { value: number; onRate: (n: number) => void; disabled?: boolean }) {
  const [hover, setHover] = useState(0)
  const shown = hover || props.value
  return (
    <div className="flex" onMouseLeave={() => setHover(0)} aria-label={`${props.value} of 5 stars`}>
      {[1, 2, 3, 4, 5].map((n) => (
        <motion.button
          key={n}
          disabled={props.disabled}
          whileTap={{ scale: 1.35 }}
          onMouseEnter={() => setHover(n)}
          onClick={() => props.onRate(n)}
          className={`px-[1px] text-[15px] leading-none transition-opacity ${
            props.value || hover ? '' : 'opacity-0 group-hover:opacity-100'
          }`}
          style={{ color: n <= shown ? 'var(--accent)' : 'color-mix(in oklab, var(--ink) 22%, transparent)' }}
          aria-label={`${n} stars`}
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
