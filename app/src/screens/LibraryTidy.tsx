import { AnimatePresence, motion } from 'motion/react'
import { useCallback, useEffect, useMemo, useState } from 'react'
import {
  cancelApply,
  getLayout,
  inApp,
  saveAliases,
  setLayout,
  tidyApply,
  tidyPreview,
  tidyUndo,
  tryLayout,
  type Aliases,
  type LayoutTrial,
  type TidyAlbum,
  type TidyPreview,
  type TidyReport,
} from '../lib/engine'
import { track, useJob } from '../lib/jobs'
import { ArtistNames } from '../components/ArtistNames'
import { DeepProgressBar } from './libraryParts'

const spring = { type: 'spring', stiffness: 380, damping: 34 } as const
const n = (v: number) => v.toLocaleString('en-IN')
const plural = (v: number, one: string, many: string) => `${n(v)} ${v === 1 ? one : many}`

/*
  How a library's folders are named, as choices. The saved rule is a path pattern; the choices
  below cover the common ones, and any other pattern is shown as your own.
*/
const ALBUM_STYLES = [
  { id: 'name-year', label: 'Taal (1999)', part: '{album} ({year})' },
  { id: 'year-name', label: '1999 - Taal', part: '{year} - {album}' },
  { id: 'name', label: 'Taal', part: '{album}' },
] as const
const SONG_STYLES = [
  { id: 'dash', label: '01 - Beat Of Passion', part: '{disc}{track} - {title}' },
  { id: 'space', label: '01 Beat Of Passion', part: '{disc}{track} {title}' },
] as const

interface Arrangement {
  language: boolean
  artist: boolean
  album: (typeof ALBUM_STYLES)[number]['id']
  song: (typeof SONG_STYLES)[number]['id']
}

function compose(a: Arrangement): string {
  return [
    '{category}',
    '{subfolder}',
    a.language && '{language}',
    a.artist && '{albumartist}',
    ALBUM_STYLES.find((s) => s.id === a.album)!.part,
    SONG_STYLES.find((s) => s.id === a.song)!.part,
  ]
    .filter(Boolean)
    .join('/')
}

/** The choices a saved pattern stands for, or null when it is one of your own. */
function parse(t: string): Arrangement | null {
  for (const language of [true, false])
    for (const artist of [true, false])
      for (const album of ALBUM_STYLES)
        for (const song of SONG_STYLES) {
          const a = { language, artist, album: album.id, song: song.id }
          if (compose(a) === t) return a
        }
  return null
}

/** "Film & Series/Hindi/A.R. Rahman/Taal (1999)/01 - x.flac" → its parts, for a breadcrumb. */
const parts = (p: string) => p.split('/').filter(Boolean)

function PathLine(props: { path: string; dim?: boolean }) {
  const ps = parts(props.path)
  return (
    <span className={`flex min-w-0 flex-wrap items-center gap-x-1.5 gap-y-0.5 text-[12.5px] ${props.dim ? 'text-ink/45' : 'text-ink/80'}`}>
      {ps.map((p, i) => (
        <span key={i} className="flex items-center gap-1.5">
          {i > 0 && <span className="text-ink/30">›</span>}
          <span className={i === ps.length - 1 ? 'font-medium' : ''}>{p}</span>
        </span>
      ))}
    </span>
  )
}

function Choice<T extends string>(props: { value: T; options: readonly { id: T; label: string }[]; onChange: (v: T) => void; name: string }) {
  return (
    <div role="radiogroup" aria-label={props.name} className="inline-flex flex-wrap rounded-full bg-ink/[0.07] p-1">
      {props.options.map((o) => (
        <button
          key={o.id}
          role="radio"
          aria-checked={props.value === o.id}
          onClick={() => props.onChange(o.id)}
          className={`relative rounded-full px-3.5 py-1.5 text-[12.5px] ${props.value === o.id ? 'text-ink' : 'text-ink/60 hover:text-ink'}`}
        >
          {props.value === o.id && (
            <motion.span layoutId={`choice-${props.name}`} transition={spring} className="absolute inset-0 rounded-full bg-ink/[0.14]" />
          )}
          <span className="relative">{o.label}</span>
        </button>
      ))}
    </div>
  )
}

function Toggle(props: { on: boolean; label: string; onChange: (on: boolean) => void }) {
  return (
    <button
      role="switch"
      aria-checked={props.on}
      onClick={() => props.onChange(!props.on)}
      title={props.on ? `Leave out the ${props.label.toLowerCase()} folder` : `Add a ${props.label.toLowerCase()} folder`}
      className={`flex items-center gap-1.5 rounded-full border px-3 py-1.5 text-[12.5px] ${
        props.on ? 'border-transparent bg-ink/[0.14] text-ink' : 'border-dashed border-ink/25 text-ink/50'
      }`}
    >
      <span aria-hidden className={`grid h-3.5 w-3.5 place-items-center rounded-full text-[9px] ${props.on ? 'bg-accent text-on-accent' : 'border border-ink/30'}`}>
        {props.on ? '✓' : ''}
      </span>
      {props.label}
    </button>
  )
}

function Card(props: { children: React.ReactNode; className?: string }) {
  return <section className={`glass rounded-[22px] px-6 py-5 ${props.className ?? ''}`}>{props.children}</section>
}

/** What is different between your library and your rules, and the way to bring them in line. */
function Changes(props: {
  root: string
  preview: TidyPreview | null
  error: string
  onChanged: () => void
  onRefresh: () => void
  /** Applied: the rule changes behind it now belong to this run. */
  onApplied: () => void
  /** Undone: put those rules back too. */
  onUndone: () => Promise<void>
}) {
  const { preview } = props
  const [open, setOpen] = useState(false)
  const [left, setLeft] = useState<Set<string>>(new Set())
  const [running, setRunning] = useState<{ done: number; total: number } | null>(null)
  const [result, setResult] = useState<TidyReport | null>(null)
  const [undone, setUndone] = useState('')
  const [error, setError] = useState('')
  const job = useJob('tidy')
  const progress = running ?? (job ? { done: job.done, total: job.total } : null)

  const doable = useMemo(() => preview?.albums.filter((a) => !a.blocked) ?? [], [preview])
  const blocked = useMemo(() => preview?.albums.filter((a) => a.blocked) ?? [], [preview])
  const chosen = doable.filter((a) => !left.has(a.id))
  const moves = chosen.reduce((s, a) => s + a.moves, 0)
  const retags = chosen.reduce((s, a) => s + a.retags, 0)

  async function apply() {
    setError('')
    setUndone('')
    setRunning({ done: 0, total: 0 })
    try {
      const ids = left.size ? chosen.map((a) => a.id) : []
      const r = await track({ id: 'tidy', label: 'Tidying your library', unit: 'songs', cancel: () => void cancelApply() }, (update) =>
        tidyApply(props.root, ids, (done, total) => {
          setRunning({ done, total })
          update(done, total)
        }),
      )
      setResult(r)
      setLeft(new Set())
      props.onApplied()
      props.onChanged()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setRunning(null)
      props.onRefresh()
    }
  }

  async function undo() {
    if (!result) return
    setError('')
    try {
      await track({ id: 'tidy', label: 'Putting your library back' }, () => tidyUndo(props.root, result.journal))
      await props.onUndone()
      setUndone('Put back as it was, and the naming and spellings with it.')
      window.setTimeout(() => setUndone(''), 6000)
      setResult(null)
      props.onChanged()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      props.onRefresh()
    }
  }

  if (progress)
    return (
      <Card>
        <DeepProgressBar deep={progress} label="Tidying your library" unit="songs" onCancel={() => void cancelApply()} />
        <p className="mt-2 text-[12.5px] text-ink/55">Files are renamed where they are; nothing is copied or deleted.</p>
      </Card>
    )

  return (
    <Card>
      {result && (
        <div className="mb-4 flex flex-wrap items-center gap-3 rounded-2xl bg-ink/[0.05] px-4 py-3 text-[13px]">
          <span className="min-w-0 flex-1 text-ink/80">
            {result.cancelled ? 'Stopped. ' : 'Done. '}
            {[result.moved && `${plural(result.moved, 'song', 'songs')} renamed`, result.retagged && `${plural(result.retagged, 'song', 'songs')} updated`]
              .filter(Boolean)
              .join(', ') || 'Nothing needed changing'}
            {result.errors.length ? `; ${plural(result.errors.length, 'file', 'files')} could not be changed and stayed as they were` : ''}.
          </span>
          <button onClick={() => void undo()} className="btn btn-sm">
            Undo
          </button>
        </div>
      )}
      {undone && <p className="mb-3 text-[13px] text-good">{undone}</p>}

      {!preview && !props.error && <p className="text-[13.5px] text-ink/55">Checking your library against your rules…</p>}
      {props.error && <p className="text-[13px] text-problem">{props.error}</p>}

      {preview && doable.length === 0 && blocked.length === 0 && (
        <div>
          <h3 className="text-[16px] font-medium">Your library matches your rules</h3>
          <p className="mt-1 text-[13px] text-ink/55">
            Change how folders are named or how an artist is written below, and what that changes shows up here first.
          </p>
        </div>
      )}

      {preview && (doable.length > 0 || blocked.length > 0) && (
        <div>
          <div className="flex flex-wrap items-start gap-4">
            <div className="min-w-0 flex-1">
              <h3 className="text-[16px] font-medium">
                {doable.length ? `${plural(doable.length, 'album', 'albums')} to bring in line with your rules` : 'Nothing can change yet'}
              </h3>
              <p className="mt-1 max-w-[64ch] text-[13px] leading-relaxed text-ink/60">
                {[moves && `${plural(moves, 'song', 'songs')} renamed or moved`, retags && `${plural(retags, 'song', 'songs')} with updated details`]
                  .filter(Boolean)
                  .join(', ')}
                {moves || retags ? '. ' : ''}
                {moves
                  ? 'Files stay on the same drive and are renamed where they are; nothing is copied or deleted, and you can undo it.'
                  : 'Only the details inside the files change; the audio is untouched, and you can undo it.'}
              </p>
            </div>
            {doable.length > 0 && (
              <button
                onClick={() => void apply()}
                disabled={!inApp() || chosen.length === 0}
                className="btn-primary btn-lg"
              >
                {chosen.length === doable.length ? 'Apply changes' : `Apply to ${plural(chosen.length, 'album', 'albums')}`}
              </button>
            )}
          </div>
          {error && <p className="mt-2 text-[13px] text-problem">{error}</p>}

          <button onClick={() => setOpen((o) => !o)} aria-expanded={open} className="mt-3 text-[13px] text-accent-ink hover:underline">
            {open ? 'Hide the albums' : 'See each album'}
          </button>
          <AnimatePresence initial={false}>
            {open && (
              <motion.div
                initial={{ height: 0, opacity: 0 }}
                animate={{ height: 'auto', opacity: 1 }}
                exit={{ height: 0, opacity: 0 }}
                className="overflow-hidden"
              >
                <ul className="mt-3 max-h-[440px] divide-y divide-ink/[0.06] overflow-y-auto pr-1">
                  {doable.map((a) => (
                    <AlbumChange
                      key={a.id}
                      a={a}
                      on={!left.has(a.id)}
                      onToggle={(on) =>
                        setLeft((s) => {
                          const next = new Set(s)
                          if (on) next.delete(a.id)
                          else next.add(a.id)
                          return next
                        })
                      }
                    />
                  ))}
                </ul>
              </motion.div>
            )}
          </AnimatePresence>
          {blocked.length > 0 && (
            <div className="mt-4 rounded-2xl bg-[color-mix(in_oklab,var(--color-fix)_10%,transparent)] px-4 py-3 text-[12.5px]">
              <div className="font-medium text-ink/85">{plural(blocked.length, 'album stays', 'albums stay')} as it is</div>
              <div className="mt-0.5 text-ink/60">Another file is already where it would go, and Venuta never writes over a file.</div>
              <ul className="mt-2 space-y-1">
                {blocked.slice(0, 6).map((a) => (
                  <li key={a.id} className="text-ink/70">
                    {a.title}: {a.blocked}
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
    </Card>
  )
}

function AlbumChange(props: { a: TidyAlbum; on: boolean; onToggle: (on: boolean) => void }) {
  const { a } = props
  const what = [
    a.set_aside && 'set aside as a duplicate',
    !a.set_aside && a.moves > 0 && (a.from_dir.split('/').slice(0, -1).join('/') === a.to_dir.split('/').slice(0, -1).join('/') ? 'renamed' : 'moved'),
    a.retags > 0 && `details updated${a.fields.length ? ` (${a.fields.slice(0, 3).map((f) => f.toLowerCase()).join(', ')})` : ''}`,
  ].filter(Boolean)
  return (
    <li className="flex items-start gap-3 py-2.5">
      <input
        type="checkbox"
        checked={props.on}
        onChange={(e) => props.onToggle(e.target.checked)}
        aria-label={`Include ${a.title}`}
        className="mt-1 h-4 w-4 accent-[var(--accent)]"
      />
      <div className="min-w-0 flex-1">
        <div className="text-[13.5px]">
          <span className="font-medium">{a.title}</span>
          <span className="text-ink/50">, {a.album_artist}</span>
          <span className="text-ink/45">: {what.join(', ')}</span>
        </div>
        {a.from_dir !== a.to_dir && (
          <div className="mt-1 grid gap-0.5">
            <PathLine path={a.from_dir} dim />
            <PathLine path={a.to_dir} />
          </div>
        )}
      </div>
    </li>
  )
}

/** How folders and files are named: choices with live examples from your own albums. */
function Naming(props: { root: string; onSaved: (previous: string) => void }) {
  const [saved, setSaved] = useState<string | null>(null)
  const [draft, setDraft] = useState<Arrangement | null>(null)
  const [trial, setTrial] = useState<LayoutTrial | null>(null)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    getLayout()
      .then((t) => {
        setSaved(t)
        setDraft(parse(t))
      })
      .catch((e) => setError(String(e)))
  }, [])

  const template = draft ? compose(draft) : saved
  const changed = !!template && !!saved && template !== saved

  // Try a changed arrangement on the library (nothing saved or moved).
  useEffect(() => {
    if (!changed || !template) return
    let live = true
    const t = window.setTimeout(() => {
      tryLayout(props.root, template)
        .then((r) => live && setTrial(r))
        .catch((e) => live && setError(String(e)))
    }, 350)
    return () => {
      live = false
      window.clearTimeout(t)
    }
  }, [changed, template, props.root])

  async function use() {
    if (!template) return
    setBusy(true)
    try {
      const previous = saved
      await setLayout(template)
      setSaved(template)
      setTrial(null)
      if (previous) props.onSaved(previous)
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy(false)
    }
  }

  if (!saved) return <Card>{error ? <p className="text-[13px] text-problem">{error}</p> : <p className="text-[13px] text-ink/55">Reading how your folders are named…</p>}</Card>

  return (
    <Card>
      <h3 className="text-[16px] font-medium">How folders and files are named</h3>
      {!draft ? (
        <div className="mt-2 text-[13px] text-ink/60">
          Your own pattern: <code className="rounded bg-ink/[0.07] px-1.5 py-0.5 text-[12px]">{saved}</code>. It is kept as you wrote it.
        </div>
      ) : (
        <div className="mt-4 grid gap-4">
          <div className="flex flex-wrap items-center gap-3">
            <span className="w-[130px] text-[13px] text-ink/60">Album folders</span>
            <Choice name="album" value={draft.album} options={ALBUM_STYLES} onChange={(album) => setDraft({ ...draft, album })} />
          </div>
          <div className="flex flex-wrap items-center gap-3">
            <span className="w-[130px] text-[13px] text-ink/60">Song files</span>
            <Choice name="song" value={draft.song} options={SONG_STYLES} onChange={(song) => setDraft({ ...draft, song })} />
          </div>
          <div className="flex flex-wrap items-center gap-3">
            <span className="w-[130px] text-[13px] text-ink/60">Folders</span>
            <span className="px-1 text-[12.5px] text-ink/75">Category</span>
            <span className="text-ink/30">›</span>
            <Toggle label="Language" on={draft.language} onChange={(language) => setDraft({ ...draft, language })} />
            <span className="text-ink/30">›</span>
            <Toggle label="Album artist" on={draft.artist} onChange={(artist) => setDraft({ ...draft, artist })} />
            <span className="text-ink/30">›</span>
            <span className="px-1 text-[12.5px] text-ink/75">Album</span>
          </div>
        </div>
      )}

      <AnimatePresence>
        {changed && (
          <motion.div initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0 }} className="mt-5 rounded-2xl bg-ink/[0.05] px-4 py-4">
            {!trial ? (
              <p className="text-[13px] text-ink/55">Trying it on your albums…</p>
            ) : (
              <>
                <div className="text-[13.5px]">
                  {trial.albums ? `${plural(trial.albums, 'album', 'albums')} would be renamed, for example:` : 'No album would change.'}
                </div>
                <ul className="mt-3 grid gap-3">
                  {trial.examples.map((e) => (
                    <li key={e.from} className="grid gap-0.5">
                      <PathLine path={e.from} dim />
                      <PathLine path={e.to} />
                    </li>
                  ))}
                </ul>
                <div className="mt-4 flex flex-wrap gap-2">
                  <button onClick={() => void use()} disabled={busy} className="btn-primary">
                    Use this naming
                  </button>
                  <button onClick={() => setDraft(parse(saved))} className="btn">
                    Keep it as it is
                  </button>
                </div>
                <p className="mt-2 text-[12px] text-ink/50">Nothing is renamed yet: the changes appear at the top, to apply when you're ready.</p>
              </>
            )}
          </motion.div>
        )}
      </AnimatePresence>
      {error && <p className="mt-3 text-[13px] text-problem">{error}</p>}
    </Card>
  )
}

/**
 * Organize, inside your library: what differs from your rules (to apply, and undo), how
 * folders are named, and how artists are written. Rules change here; files change only when
 * you apply.
 */
export function LibraryTidy(props: { root: string; onChanged: () => void }) {
  const [preview, setPreview] = useState<TidyPreview | null>(null)
  const [error, setError] = useState('')
  const [names, setNames] = useState(false)
  // Rules as they were before the changes waiting (pending) or last applied (applied).
  const [pending, setPending] = useState<{ layout?: string; aliases?: Aliases }>({})
  const [applied, setApplied] = useState<{ layout?: string; aliases?: Aliases }>({})
  const [namingKey, setNamingKey] = useState(0)
  const [namesKey, setNamesKey] = useState(0)

  const refresh = useCallback(() => {
    tidyPreview(props.root)
      .then((p) => {
        setError('')
        setPreview(p)
      })
      .catch((e) => setError(e instanceof Error ? e.message : String(e)))
  }, [props.root])
  useEffect(() => refresh(), [refresh])

  return (
    <div className="grid gap-4">
      <Changes
        root={props.root}
        preview={preview}
        error={error}
        onChanged={props.onChanged}
        onRefresh={refresh}
        onApplied={() => {
          setApplied(pending)
          setPending({})
        }}
        onUndone={async () => {
          if (applied.layout) await setLayout(applied.layout)
          if (applied.aliases) await saveAliases(applied.aliases)
          setApplied({})
          setNamingKey((k) => k + 1)
          setNamesKey((k) => k + 1)
        }}
      />
      <Naming
        key={namingKey}
        root={props.root}
        onSaved={(previous) => {
          setPending((p) => ({ layout: p.layout ?? previous, aliases: p.aliases }))
          setPreview(null)
          refresh()
        }}
      />
      <Card>
        <button onClick={() => setNames((o) => !o)} aria-expanded={names} className="flex w-full items-center gap-3 text-left">
          <span className="flex-1">
            <span className="block text-[16px] font-medium">How artists are written</span>
            <span className="mt-0.5 block text-[13px] text-ink/55">One spelling per person everywhere; click a name to change it.</span>
          </span>
          <span className={`text-ink/50 transition-transform ${names ? 'rotate-90' : ''}`}>›</span>
        </button>
        {names && (
          <div className="mt-4">
            <ArtistNames
              key={namesKey}
              onSaved={(previous) => {
                setPending((p) => ({ layout: p.layout, aliases: p.aliases ?? previous }))
                setPreview(null)
                refresh()
              }}
            />
          </div>
        )}
      </Card>
    </div>
  )
}
