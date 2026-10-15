import { motion } from 'motion/react'
import { useMemo, useState } from 'react'
import { AvailabilityBadge } from '../../components/AvailabilityBadge'
import { IconPlay } from '../../components/Icons'
import { RemoteArt } from '../../components/RemoteArt'
import type { OnlineEntry, OnlineTrack, PlaylistEntry } from '../../lib/engine'
import { useEscape } from '../../lib/layers'
import { clock, onlineEntryFrom, playItems, toRemoteItem } from '../../lib/player'
import { useOnlineSearch } from '../listen/onlineSearch'
import { rankOnlineTracks } from '../../lib/searchRank'
import { plural } from './common'

/*
  Choosing which online song a playlist's missing song is: the top few catalog hits for its
  words, each with its length beside the one the playlist remembers, so a wrong version is
  easy to see. Nothing is chosen for you; Cancel leaves the song as one to get.

  For one song (its menu) a choice is used at once. For several ("Fill missing online…") the
  choices are collected one song at a time and saved together, with Undo.
*/

const TOP = 5

/** The words to look a missing song up by: artist and title. */
const wordsOf = (r: PlaylistEntry) => [r.artist, r.title].filter(Boolean).join(' ')

/** "Same length", "12 s longer": how a hit's length compares with the song the playlist wants. */
function lengthNote(wanted: number, found: number | null): { text: string; close: boolean } | null {
  if (!wanted || !found) return null
  const d = Math.round((found - wanted) / 1000)
  if (Math.abs(d) <= 3) return { text: 'Same length', close: true }
  const by = Math.abs(d) >= 90 ? `${Math.round(Math.abs(d) / 60)} min` : `${Math.abs(d)} s`
  return { text: `${by} ${d > 0 ? 'longer' : 'shorter'}`, close: false }
}

export function OnlinePick<R extends PlaylistEntry & { key: string }>(props: {
  root: string
  /** The songs to find (to-get rows), in order. */
  rows: R[]
  /** 'use': put the chosen song in the playlist. 'play': just play it now. */
  mode: 'use' | 'play'
  /** The choices, by row key (never called with none). */
  onUse: (chosen: [R, OnlineEntry][]) => void
  onClose: () => void
}) {
  const [at, setAt] = useState(0)
  const [chosen, setChosen] = useState<[R, OnlineEntry][]>([])
  const row = props.rows[at]
  const [q, setQ] = useState(() => (row ? wordsOf(row) : ''))
  const { results, error, loading, resultsFor } = useOnlineSearch(q, 'pick')
  // Ranked again with the length the playlist remembers: a hit that close is most likely it.
  const want = row?.duration_ms
  const hits = useMemo(
    () => (resultsFor === q.trim() ? rankOnlineTracks(q, results?.tracks ?? [], { wantDurationMs: want }).slice(0, TOP) : []),
    [results, resultsFor, q, want],
  )
  const many = props.rows.length > 1
  useEscape(true, props.onClose)

  if (!row) return null

  const finish = (all: [R, OnlineEntry][]) => {
    if (all.length) props.onUse(all)
    props.onClose()
  }
  const next = (all: [R, OnlineEntry][]) => {
    if (at + 1 < props.rows.length) {
      setChosen(all)
      setAt(at + 1)
      setQ(wordsOf(props.rows[at + 1]))
    } else finish(all)
  }
  const choose = (t: OnlineTrack) => {
    if (props.mode === 'play') {
      void playItems(props.root, [toRemoteItem(t)], 0, { kind: 'search', ref: q.trim(), label: `Online: ${t.title}` })
      return props.onClose()
    }
    next([...chosen, [row, onlineEntryFrom(t)]])
  }
  const listen = (t: OnlineTrack) => void playItems(props.root, [toRemoteItem(t)], 0, { kind: 'search', ref: q.trim(), label: `Online: ${t.title}` })

  return (
    <motion.div
      className="fixed inset-0 z-50 flex items-start justify-center bg-[var(--scrim)] pt-[12vh]"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onMouseDown={props.onClose}
    >
      <motion.div
        role="dialog"
        aria-label={props.mode === 'play' ? 'Play online' : 'Choose the online song'}
        className="popover flex max-h-[70vh] w-[min(560px,92vw)] flex-col overflow-hidden rounded-[18px]"
        initial={{ y: -10, opacity: 0 }}
        animate={{ y: 0, opacity: 1 }}
        exit={{ y: -6, opacity: 0 }}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="border-b border-edge px-4 pb-3 pt-4">
          {many && (
            <div className="mb-1 text-[11.5px] font-medium uppercase tracking-[0.06em] text-ink/45">
              Song {at + 1} of {props.rows.length}
            </div>
          )}
          <div className="text-[12.5px] leading-relaxed text-ink/60">
            The playlist wants “{row.title}”{row.artist ? ` by ${row.artist}` : ''}
            {row.album ? ` (${row.album})` : ''}
            {row.duration_ms ? `, ${clock(row.duration_ms)}` : ''}.{' '}
            {props.mode === 'play' ? 'Which online song should play?' : 'Which online song is it?'}
          </div>
          <input
            autoFocus
            onFocus={(e) => e.currentTarget.select()}
            value={q}
            onChange={(e) => setQ(e.target.value)}
            data-escape="layer"
            aria-label="Search the online catalog"
            className="mt-1.5 w-full bg-transparent text-[16px] outline-none placeholder:text-ink/40"
            placeholder="Song and artist"
          />
        </div>
        <div className="fade-y min-h-0 flex-1 overflow-y-auto p-1.5">
          {loading && !hits.length ? (
            <p className="px-3 py-6 text-center text-[13px] text-ink/50">Searching the online catalog…</p>
          ) : error ? (
            <p role="status" className="px-3 py-6 text-center text-[13px] text-ink/55">
              {error}
            </p>
          ) : !hits.length ? (
            <p className="px-3 py-6 text-center text-[13px] text-ink/55">Nothing in the online catalog for these words. Try fewer, or the artist’s name.</p>
          ) : (
            <div className={`grid gap-0.5 transition-opacity ${loading ? 'opacity-55' : ''}`}>
              {hits.map((t) => {
                const len = lengthNote(row.duration_ms, t.duration_ms)
                return (
                  <div key={`${t.provider}:${t.track_id}`} className="group flex items-center gap-3 rounded-xl px-2.5 py-2 transition-colors hover:bg-ink/[0.07]">
                    <button
                      onClick={() => listen(t)}
                      aria-label={`Listen to ${t.title}`}
                      title="Listen"
                      className="relative h-10 w-10 shrink-0 overflow-hidden rounded-lg bg-ink/[0.07] text-ink/30"
                    >
                      <RemoteArt url={t.artwork_url} fallback="♪" imgClassName="h-full w-full object-cover" />
                      <span className="absolute inset-0 grid place-items-center bg-black/40 text-white opacity-0 transition-opacity group-hover:opacity-100">
                        <IconPlay width={14} height={14} />
                      </span>
                    </button>
                    <button onClick={() => choose(t)} className="flex min-w-0 flex-1 items-center gap-3 text-left">
                      <span className="min-w-0 flex-1">
                        <span className="flex min-w-0 items-center gap-1.5">
                          <span className="truncate text-[14px]">{t.title}</span>
                          <AvailabilityBadge availability="online" />
                        </span>
                        <span className="block truncate text-[12px] text-ink/55">{[t.artists.join(', '), t.album].filter(Boolean).join(' · ')}</span>
                      </span>
                      <span className="shrink-0 text-right">
                        <span className="block text-[12.5px] tabular-nums text-ink/60">{t.duration_ms ? clock(t.duration_ms) : ''}</span>
                        {len && <span className={`block text-[11px] ${len.close ? 'text-good' : 'text-ink/45'}`}>{len.text}</span>}
                      </span>
                      <span className="btn btn-sm shrink-0 opacity-0 transition-opacity group-hover:opacity-100">{props.mode === 'play' ? 'Play' : 'Use'}</span>
                    </button>
                  </div>
                )
              })}
            </div>
          )}
        </div>
        <div className="flex items-center gap-2 border-t border-edge px-4 py-2.5">
          <span className="text-[12px] text-ink/50">
            {many && chosen.length > 0 ? `${plural(chosen.length, 'song')} chosen` : props.mode === 'use' ? 'Only its id is kept, never a stream address.' : ''}
          </span>
          <span className="ml-auto" />
          {many && (
            <button onClick={() => next(chosen)} className="btn-ghost btn-sm" title="Leave this one as a song to get">
              Skip
            </button>
          )}
          {many && chosen.length > 0 && (
            <button onClick={() => finish(chosen)} className="btn btn-sm">
              Keep {chosen.length} and stop
            </button>
          )}
          <button onClick={props.onClose} className="btn-ghost btn-sm">
            Cancel
          </button>
        </div>
      </motion.div>
    </motion.div>
  )
}
