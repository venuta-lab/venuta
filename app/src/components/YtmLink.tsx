import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useMemo, useState } from 'react'
import { ytmLinkChoose, ytmLinkReview, ytmLinkStart, ytmUnlink, type OnlineSuggestion, type YtmReviewRow, type YtmSyncReport } from '../lib/engine'
import { when } from '../lib/format'
import { useEscape } from '../lib/layers'
import { notify } from '../lib/notify'
import { onlineProblem } from '../lib/online'
import { useYtmPart, useYtmStatus, ytmLibraryChanged } from '../lib/ytmLibrary'
import { pauseWords, standWords, syncWords } from '../lib/ytmLinkWords'
import { clearPause, reloadLink, syncLink, useYtmLinkState } from '../lib/ytmLinks'
import { confirmAsk } from './Confirm'
import { RemoteArt } from './RemoteArt'
import { Switch } from './ui'

/*
  "Also on YouTube Music" on a playlist's page: a switch, and once on, how it stands (synced
  when, waiting and why, files of yours not there yet) with what you can do about it. Turning
  it on asks where (a new playlist there, or one of yours) and goes through your files first:
  each one you say is a song there goes across, one you tick goes to your uploads, the rest stay
  in Venuta only. Turning it off unlinks; both playlists stay as they are.
*/

export function YtmLinkBar({ root, name }: { root: string; name: string }) {
  const { signed_in, expired } = useYtmStatus()
  const s = useYtmLinkState(root, name, true)
  const [dialog, setDialog] = useState<Mode | null>(null)
  const [showHistory, setShowHistory] = useState(false)
  const link = s.link
  if (link === undefined) return null
  if (!link && !signed_in) return null

  const unlink = async () => {
    const ok = await confirmAsk({
      title: `Unlink “${name}” from YouTube Music?`,
      text: 'Both playlists stay as they are; changes stop going between them.',
      confirm: 'Unlink',
    })
    if (!ok) return
    try {
      await ytmUnlink(name)
      clearPause(name)
      await reloadLink(root, name)
      notify(`“${name}” isn’t linked any more. Both playlists stay as they were.`)
    } catch (e) {
      notify(String(e))
    }
  }
  const p = s.paused ?? (link?.held ? ({ kind: 'held' } as const) : null)
  const forceLabel = p?.kind === 'held' ? 'Sync again' : p?.kind === 'many_gone' ? 'Sync anyway' : null
  const uploading = Object.keys(link?.uploading ?? {}).length

  return (
    <div className="mt-4 max-w-[640px] rounded-2xl bg-ink/[0.04] px-4 py-3">
      <div className="flex items-start gap-3">
        <Switch on={!!link} onChange={(on) => (on ? setDialog('link') : void unlink())} label="Also on YouTube Music" disabled={!link && (!signed_in || expired)} />
        <div className="min-w-0 flex-1">
          <p className="text-[13.5px] font-medium">
            Also on YouTube Music{link?.title && link.title !== name ? <span className="font-normal text-ink/60"> as “{link.title}”</span> : null}
          </p>
          <p role="status" className="mt-0.5 text-[12.5px] leading-relaxed text-ink/60">
            {!link
              ? 'Keep this playlist the same on your YouTube Music account, both ways.'
              : !signed_in
                ? 'Sign in to YouTube Music to keep it the same there.'
                : expired
                  ? 'YouTube Music needs you to sign in again (Settings, Online).'
                  : s.syncing
                    ? 'Syncing…'
                    : s.problem
                      ? s.problem
                      : p
                        ? pauseWords(name, p)
                        : `${link.last_sync ? `Synced ${when(link.last_sync * 1000).toLowerCase()}.` : 'Not synced yet.'} ${standWords(link.there, link.only_here)}`}
          </p>
          {link && signed_in && !expired && (
            <div className="mt-2 flex flex-wrap items-center gap-2">
              {forceLabel ? (
                <button
                  onClick={() => void syncLink(root, name, { force: true, gone: p?.kind === 'many_gone' ? p.side : undefined })}
                  disabled={s.syncing}
                  className="btn-primary btn-sm"
                >
                  {forceLabel}
                </button>
              ) : p?.kind === 'other_account' ? null : (
                <button onClick={() => void syncLink(root, name)} disabled={s.syncing} className="btn btn-sm">
                  Sync now
                </button>
              )}
              {link.unmatched > 0 && (
                <button onClick={() => setDialog('review')} className="btn-ghost btn-sm">
                  {link.unmatched === 1 ? '1 file of yours isn’t there yet' : `${link.unmatched.toLocaleString('en-IN')} files of yours aren’t there yet`}
                </button>
              )}
              {(Object.keys(link.matches).length > 0 || link.local_only.length > 0) && (
                <button onClick={() => setDialog('matches')} className="btn-ghost btn-sm">
                  Check matches
                </button>
              )}
              {!!link.history?.length && (
                <button onClick={() => setShowHistory((o) => !o)} aria-expanded={showHistory} className="btn-ghost btn-sm">
                  {showHistory ? 'Hide what syncing did' : 'What syncing did'}
                </button>
              )}
            </div>
          )}
          {uploading > 0 && (
            <p className="mt-1.5 text-[12px] text-ink/50">
              {uploading === 1 ? '1 upload is' : `${uploading} uploads are`} still being taken in by YouTube Music; each joins the playlist there once it shows.
            </p>
          )}
          {showHistory && link?.history && (
            <ul className="mt-2 grid gap-1 text-[12.5px] text-ink/60">
              {link.history.slice(0, 8).map((h) => (
                <li key={h.at}>
                  <span className="text-ink/45">{when(h.at * 1000)}: </span>
                  {syncWords(name, h).replace(/^“[^”]*” is the same on YouTube Music now: /, '') || 'Nothing changed.'}
                  {h.problem ? ` Stopped part way: ${h.problem}` : ''}
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
      <AnimatePresence>{dialog && <YtmLinkDialog key="dialog" root={root} name={name} mode={dialog} onClose={() => setDialog(null)} />}</AnimatePresence>
    </div>
  )
}

type Choice = 'current' | 'match' | 'upload' | 'keep' | 'uploading'
type Mode = 'link' | 'review' | 'matches'

/** What a file was said to be before (Check matches), else the first guess. */
function startChoice(r: YtmReviewRow): Choice {
  if (r.uploading) return 'uploading'
  if (r.current_id) return 'current'
  if (r.kept_here) return 'keep'
  return r.found ? 'match' : 'keep'
}

/** Linking, answering about files of yours added since, or checking every file's match. */
function YtmLinkDialog(props: { root: string; name: string; mode: Mode; onClose: () => void }) {
  const { root, name, mode } = props
  const [where, setWhere] = useState<'new' | 'yours'>('new')
  const [privacy, setPrivacy] = useState('PRIVATE')
  const [target, setTarget] = useState('')
  const [rows, setRows] = useState<YtmReviewRow[] | null>(null)
  const [choice, setChoice] = useState<Record<string, Choice>>({})
  const [problem, setProblem] = useState('')
  const [busy, setBusy] = useState(false)
  useEscape(!busy, props.onClose)
  const { value: playlists } = useYtmPart<OnlineSuggestion[]>('playlists', mode === 'link')
  const yours = useMemo(
    () =>
      (playlists ?? []).flatMap((s) =>
        s.kind === 'playlist' && !/^(VL)?(LM|SE)$/.test(s.collection.collection_id) ? [s.collection] : [],
      ),
    [playlists],
  )

  useEffect(() => {
    let live = true
    ytmLinkReview(root, name, mode === 'matches')
      .then((r) => {
        if (!live) return
        setRows(r)
        // A song found there is taken; nothing is uploaded unless you tick it.
        setChoice(Object.fromEntries(r.map((x) => [x.key, startChoice(x)])))
      })
      .catch((e) => live && (setProblem(onlineProblem(e)), setRows([])))
    return () => {
      live = false
    }
  }, [root, name, mode])

  const counts = useMemo(() => {
    const c = { current: 0, match: 0, upload: 0, keep: 0, uploading: 0 }
    for (const v of Object.values(choice)) c[v]++
    return c
  }, [choice])

  const go = async () => {
    if (!rows) return
    setBusy(true)
    const choices = {
      matches: rows.flatMap((r) => (choice[r.key] === 'match' && r.found ? [[r.key, r.found.track_id] as [string, string]] : [])),
      // Kept here only: told again only when it was something else before.
      upload: rows.flatMap((r) => (choice[r.key] === 'upload' ? [r.key] : [])),
      local_only: rows.flatMap((r) => (choice[r.key] === 'keep' && (mode !== 'matches' || !r.kept_here) ? [r.key] : [])),
    }
    try {
      const report: YtmSyncReport =
        mode === 'link' ? await ytmLinkStart(root, name, where === 'yours' ? target || null : null, privacy, choices) : await ytmLinkChoose(root, name, choices)
      await reloadLink(root, name)
      ytmLibraryChanged(['playlists'])
      if (report.note.undo) window.dispatchEvent(new Event('venuta:playlists-changed'))
      const said = syncWords(name, report.note)
      notify(
        [mode === 'link' ? `Linked “${name}” to YouTube Music.` : 'Saved your choices.', said.replace(/^“[^”]*” is the same on YouTube Music now: /, 'Synced: '), report.uploaded ? `${report.uploaded} sent to your uploads.` : '']
          .filter(Boolean)
          .join(' '),
      )
      if (report.upload_problems.length) notify(`Couldn't upload ${report.upload_problems.join('; ')}`)
      if (report.paused) notify(pauseWords(name, report.paused))
      props.onClose()
    } catch (e) {
      setProblem(onlineProblem(e))
      setBusy(false)
    }
  }

  const pick = (key: string, c: Choice) => setChoice((m) => ({ ...m, [key]: c }))
  const ready = rows !== null && !(mode === 'link' && where === 'yours' && !target)

  return (
    <motion.div
      className="fixed inset-0 z-[60] grid place-items-center bg-[var(--scrim)] p-6"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onMouseDown={() => !busy && props.onClose()}
    >
      <motion.div
        role="dialog"
        aria-label={mode === 'link' ? `Link ${name} to YouTube Music` : mode === 'review' ? `Files of ${name} not on YouTube Music` : `Matches of ${name} on YouTube Music`}
        className="popover flex max-h-[86vh] w-[min(640px,94vw)] flex-col rounded-[20px] p-6"
        initial={{ y: 10, opacity: 0 }}
        animate={{ y: 0, opacity: 1 }}
        exit={{ y: 6, opacity: 0 }}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <h2 className="text-[17px] font-semibold leading-snug">
          {mode === 'link' ? `Also on YouTube Music: “${name}”` : mode === 'review' ? 'Your files not on YouTube Music yet' : `Your files in “${name}” and YouTube Music`}
        </h2>
        <p className="mt-1.5 text-[13px] leading-relaxed text-ink/60">
          {mode === 'link'
            ? 'From now on, songs you add or take out here change there too, and changes made there come here. Your files never leave your library.'
            : mode === 'review'
              ? 'Choose, file by file, what each one is on YouTube Music.'
              : 'Each file of yours, and the song it is on YouTube Music. Change one that’s wrong; a file you keep here only is taken out there.'}
        </p>

        {mode === 'link' && (
          <div className="mt-4 grid gap-2 text-[13.5px]">
            <label className="flex items-center gap-2.5">
              <input type="radio" name="where" checked={where === 'new'} onChange={() => setWhere('new')} />
              A new playlist there, named “{name}”
              {where === 'new' && (
                <select value={privacy} onChange={(e) => setPrivacy(e.target.value)} aria-label="Who can see it" className="field ml-auto h-8 rounded-full px-3 text-[13px]">
                  <option value="PRIVATE">Private</option>
                  <option value="UNLISTED">Unlisted</option>
                  <option value="PUBLIC">Public</option>
                </select>
              )}
            </label>
            <label className="flex items-center gap-2.5">
              <input type="radio" name="where" checked={where === 'yours'} onChange={() => setWhere('yours')} disabled={!yours.length} />
              One of your playlists there
              {where === 'yours' && (
                <select value={target} onChange={(e) => setTarget(e.target.value)} aria-label="Which playlist" className="field ml-auto h-8 max-w-[260px] rounded-full px-3 text-[13px]">
                  <option value="">Choose…</option>
                  {yours.map((c) => (
                    <option key={c.collection_id} value={c.collection_id.replace(/^VL/, '')}>
                      {c.title}
                    </option>
                  ))}
                </select>
              )}
            </label>
            {where === 'yours' && <p className="text-[12px] text-ink/50">The two are put together the first time: songs from each go to the other, and nothing is taken out of either.</p>}
          </div>
        )}

        <div className="mt-5 min-h-0 flex-1 overflow-y-auto">
          {rows === null ? (
            <p className="py-4 text-[13px] text-ink/55">Looking for your files on YouTube Music…</p>
          ) : !rows.length ? (
            <p className="py-2 text-[13px] text-ink/55">{mode === 'link' ? 'Every song here is on YouTube Music already.' : 'Nothing to ask about.'}</p>
          ) : (
            <>
              <p className="text-[12.5px] text-ink/55">
                {rows.length === 1 ? '1 file of yours' : `${rows.length.toLocaleString('en-IN')} files of yours`} in this playlist: {counts.current + counts.match} go across as a song there, {counts.upload + counts.uploading} to your uploads, {counts.keep} stay here only.
              </p>
              <ul className="mt-2 grid gap-1">
                {rows.map((r) => (
                  <li key={r.key} className="flex items-center gap-3 rounded-xl px-2 py-2 hover:bg-ink/[0.04]">
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-[13.5px] font-medium">{r.title}</p>
                      <p className="truncate text-[12px] text-ink/55">{r.artist || r.album}</p>
                    </div>
                    <Shown row={r} choice={choice[r.key]} />
                    {choice[r.key] === 'uploading' ? (
                      <span className="shrink-0 text-[12px] text-ink/50">On its way to your uploads</span>
                    ) : (
                      <select
                        value={choice[r.key]}
                        onChange={(e) => pick(r.key, e.target.value as Choice)}
                        aria-label={`What ${r.title} is on YouTube Music`}
                        className="field h-8 shrink-0 rounded-full px-3 text-[12.5px]"
                      >
                        {r.current_id && <option value="current">The song chosen before</option>}
                        {r.found && r.found.track_id !== r.current_id && <option value="match">{r.current_id ? 'The song found now' : 'This song'}</option>}
                        {r.uploadable && <option value="upload">Upload it</option>}
                        <option value="keep">Keep here only</option>
                      </select>
                    )}
                  </li>
                ))}
              </ul>
              {counts.upload > 0 && <p className="mt-2 text-[12px] leading-relaxed text-ink/50">Uploads are yours alone on YouTube Music; it takes a while before each one shows up and joins the playlist there.</p>}
            </>
          )}
        </div>

        {problem && <p className="mt-3 text-[13px] text-problem">{problem}</p>}
        <div className="mt-5 flex justify-end gap-2">
          <button onClick={props.onClose} disabled={busy} className="btn-ghost">
            Cancel
          </button>
          <button onClick={() => void go()} disabled={!ready || busy} className="btn-primary">
            {busy ? (mode === 'link' ? 'Linking…' : 'Saving…') : mode === 'link' ? 'Link playlist' : 'Save choices'}
          </button>
        </div>
      </motion.div>
    </motion.div>
  )
}

/** The song on YouTube Music a file goes across as, for the choice made. */
function Shown({ row, choice }: { row: YtmReviewRow; choice: Choice }) {
  const o = choice === 'current' ? row.current : choice === 'match' ? row.found : null
  if (!o) return null
  return (
    <div className="flex min-w-0 max-w-[200px] items-center gap-2" title={`On YouTube Music: ${o.title}, ${(o.artists ?? []).join(', ')}`}>
      <span className="h-8 w-8 shrink-0 overflow-hidden rounded-md bg-ink/10">
        <RemoteArt url={o.artwork_url ?? null} seed={o.title} size={32} imgClassName="h-full w-full object-cover" />
      </span>
      <span className="min-w-0">
        <span className="block truncate text-[12.5px]">{o.title}</span>
        <span className="block truncate text-[11.5px] text-ink/50">{(o.artists ?? []).join(', ')}</span>
      </span>
    </div>
  )
}
