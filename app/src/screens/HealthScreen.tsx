import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useMemo, useState } from 'react'
import { review, SPECTRAL_ISSUES, type AlbumSummary, type AlbumView, type HealthReport, type Issue } from '../lib/engine'
import { FormatBar, IssueRow, SpectrogramView } from './LibraryScreen'

/*
  Health: how complete and how sound your library is, and the tools that look after it.
  Browsing lives in Music; questions live in the Inbox.
*/

const fmt = (n: number) => n.toLocaleString('en-IN')

/** What fixes each finding, so every item says what to do about it. */
type Fix = 'build' | 'lookup' | 'inbox' | 'copy' | 'info'
const FIX_OF: Record<string, Fix> = {
  no_replaygain: 'build',
  art_progressive: 'build',
  junk_tags: 'build',
  crowded_album_artist: 'build',
  name_variants: 'build',
  long_paths: 'build',
  mixed_album_tags: 'build',
  missing_album_artist: 'build',
  missing_genre: 'build',
  missing_composer: 'lookup',
  rerelease_dates: 'lookup',
  no_lyrics: 'lookup',
  art_small: 'lookup',
  duplicate_audio: 'inbox',
  duplicate_recording: 'inbox',
  lossy_files: 'copy',
  upsampled_hires: 'copy',
  padded_24bit: 'copy',
  limited_highs: 'copy',
  damaged_audio: 'copy',
  incomplete_albums: 'copy',
  no_checksum: 'info',
}

const GROUPS: { fix: Fix; title: string; text: string }[] = [
  {
    fix: 'copy',
    title: 'Only a better copy fixes these',
    text: 'Problems in the sound files themselves. They are copied as they are; the want-list in Playlists is the place to find proper releases.',
  },
  {
    fix: 'inbox',
    title: 'Handled in the Inbox',
    text: 'Duplicates are compared and set aside with proof there.',
  },
  {
    fix: 'lookup',
    title: 'Look up details online finds more',
    text: 'Credits, original years, lyrics and sharper covers come from free sources. What is found is added when you build.',
  },
  {
    fix: 'build',
    title: 'Fixed when you build',
    text: 'The new library gets these right on its own. Nothing to do.',
  },
  { fix: 'info', title: 'Good to know', text: '' },
]

/** Completeness of one detail, from the matching finding. */
/** Completeness of one detail, by album, and what in the plan fills it. */
const has = (a: AlbumView, field: string) =>
  a.changes.some((c) => c.field.toUpperCase() === field && c.after.some((v) => v.trim() !== ''))
const METERS: { label: string; issue: string; filled: (a: AlbumView) => boolean }[] = [
  { label: 'Composer', issue: 'missing_composer', filled: (a) => has(a, 'COMPOSER') },
  { label: 'Lyrics', issue: 'no_lyrics', filled: (a) => has(a, 'LYRICS') },
  { label: 'Genre', issue: 'missing_genre', filled: (a) => has(a, 'GENRE') },
  { label: 'Album artist', issue: 'missing_album_artist', filled: (a) => has(a, 'ALBUMARTIST') },
  { label: 'Sharp cover (1000 px or more)', issue: 'art_small', filled: (a) => !!a.cover_upgrade },
  { label: 'Cover every player shows', issue: 'art_progressive', filled: (a) => a.art.kind === 'reencode' },
  { label: 'Volume levelling', issue: 'no_replaygain', filled: (a) => has(a, 'REPLAYGAIN_ALBUM_GAIN') },
]

const FIX_LABEL: Record<Fix, string> = {
  build: 'Fixed when you build',
  lookup: 'Look up online',
  inbox: 'In the Inbox',
  copy: 'Needs a better copy',
  info: '',
}

export function HealthScreen(props: {
  root: string
  report: HealthReport | null
  busy: string
  actions: React.ReactNode
  progress: React.ReactNode
}) {
  const { report } = props
  const [open, setOpen] = useState<string | null>(null)
  const [spectrum, setSpectrum] = useState<AlbumSummary | null>(null)
  const albumsByDir = useMemo(() => new Map((report?.albums ?? []).map((a) => [a.dir, a])), [report])
  const s = report?.summary
  const issues = report?.issues ?? []
  const byId = new Map(issues.map((i) => [i.id, i]))
  // The plan, to show where each detail lands after you build.
  const [plan, setPlan] = useState<AlbumView[] | null>(null)
  useEffect(() => {
    if (!props.root || !report) return
    let live = true
    review(props.root)
      .then((r) => live && setPlan(r.plan.albums))
      .catch(() => {})
    return () => {
      live = false
    }
  }, [props.root, report])
  const planBySource = useMemo(() => {
    const m = new Map<string, AlbumView[]>()
    for (const a of plan ?? []) m.set(a.source_dir, [...(m.get(a.source_dir) ?? []), a])
    return m
  }, [plan])

  return (
    <div className="mx-auto max-w-[980px] pb-16">
      <header className="flex flex-wrap items-end gap-x-6 gap-y-3 pb-2 pt-6">
        <div className="min-w-0 flex-1">
          <h1 className="text-[40px] font-semibold leading-none tracking-[-0.035em]">Health</h1>
          <p className="mt-2.5 max-w-[62ch] text-[14px] text-ink/60">
            {s
              ? `${fmt(s.albums)} albums, ${fmt(s.tracks)} songs, ${Math.round(s.hours)} hours, ${(s.bytes / 1e9).toFixed(1)} GB. How complete and how sound they are, and what fixes the rest.`
              : props.busy || 'Reading your music…'}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">{props.actions}</div>
      </header>
      {props.progress}

      {s && (
        <section aria-label="How complete" className="mt-8">
          <h2 className="mb-3 text-[15px] font-medium text-ink/80">How complete the details are</h2>
          <div className="glass grid gap-x-8 gap-y-3 rounded-[22px] px-6 py-5 md:grid-cols-2">
            {METERS.map((m) => {
              const issue = byId.get(m.issue)
              const total = s.albums
              const missing = issue ? issue.albums.length : 0
              const pct = total ? Math.max(0, Math.round(((total - missing) / total) * 100)) : 100
              const fix = FIX_OF[m.issue]
              // Albums the build fills in (every destination album from that folder gets it).
              const fixed = plan
                ? (issue?.albums ?? []).filter((d) => {
                    const dest = planBySource.get(d)
                    return !!dest?.length && dest.every(m.filled)
                  }).length
                : 0
              const after = total ? Math.min(100, Math.round(((total - missing + fixed) / total) * 100)) : 100
              return (
                <button
                  key={m.label}
                  onClick={() => issue && setOpen(open === issue.id ? null : issue.id)}
                  className="group text-left"
                  disabled={!issue}
                >
                  <div className="flex items-baseline gap-2 text-[13.5px]">
                    <span className="flex-1">{m.label}</span>
                    <span className="tabular-nums text-ink/70">
                      {pct}%{after > pct && <span className="text-good"> → {after}% after you build</span>}
                    </span>
                  </div>
                  <div className="relative mt-1.5 h-1.5 overflow-hidden rounded-full bg-ink/10">
                    <motion.div
                      className="absolute inset-y-0 left-0 rounded-full"
                      style={{ background: 'color-mix(in oklab, var(--color-good) 45%, transparent)' }}
                      initial={{ width: 0 }}
                      animate={{ width: `${after}%` }}
                      transition={{ type: 'spring', stiffness: 70, damping: 20 }}
                    />
                    <motion.div
                      className="absolute inset-y-0 left-0 rounded-full"
                      style={{ background: pct === 100 ? 'var(--color-good)' : 'var(--accent)' }}
                      initial={{ width: 0 }}
                      animate={{ width: `${pct}%` }}
                      transition={{ type: 'spring', stiffness: 90, damping: 20 }}
                    />
                  </div>
                  <div className="mt-1 text-[11.5px] text-ink/45 group-hover:text-ink/70">
                    {(() => {
                      const left = missing - fixed
                      const n = (x: number) => `${fmt(x)} ${x === 1 ? 'album' : 'albums'}`
                      if (!missing) return 'Complete.'
                      if (!fixed) return `${n(missing)} without. ${FIX_LABEL[fix]}.`
                      if (!left) return `${n(missing)} without; the build fills them all.`
                      return `${n(missing)} without; the build fills ${fmt(fixed)}, ${fmt(left)} left${fix === 'lookup' ? ' (looking up again may find more)' : ''}.`
                    })()}
                  </div>
                </button>
              )
            })}
          </div>
        </section>
      )}

      {s && <FormatBar formats={s.formats} total={s.tracks} />}

      {GROUPS.map((g) => {
        const list = issues.filter((i: Issue) => (FIX_OF[i.id] ?? 'info') === g.fix)
        if (!list.length) return null
        return (
          <section key={g.fix} className="mt-10">
            <h2 className="text-[15px] font-medium text-ink/80">{g.title}</h2>
            {g.text && <p className="mt-1 max-w-[66ch] text-[13px] text-ink/55">{g.text}</p>}
            <div className="mt-3 flex flex-col gap-2">
              {list.map((issue) => (
                <IssueRow
                  key={issue.id}
                  issue={issue}
                  open={open === issue.id}
                  onToggle={() => setOpen(open === issue.id ? null : issue.id)}
                  albums={albumsByDir}
                  onAlbum={SPECTRAL_ISSUES.has(issue.id) ? setSpectrum : undefined}
                />
              ))}
            </div>
          </section>
        )
      })}

      <AnimatePresence>{spectrum && <SpectrogramView root={props.root} album={spectrum} onClose={() => setSpectrum(null)} />}</AnimatePresence>
    </div>
  )
}
