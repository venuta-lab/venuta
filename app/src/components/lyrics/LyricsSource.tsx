import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useRef, useState } from 'react'
import { lyricsCheck, lyricsChoose, lyricsOffset, type LyricsSourceStatus } from '../../lib/engine'
import { useEscape } from '../../lib/layers'
import { lyricsAsk, lyricsChoiceChanged, type Lyrics } from '../../lib/lyrics'
import type { QueueItem } from '../../lib/player'

/*
  Under lyrics found online: where they came from, and a way to put them right for this song.
  Automatic takes the first source in your order with timed lyrics; any other source can be
  chosen instead (each says what it has for this song); the timing can be moved earlier or
  later; and a song named wrongly can be asked about by its real title. Choices are kept per
  song (Settings has the order of the sources).
*/

const spring = { type: 'spring', stiffness: 480, damping: 36 } as const
const STEP = 250

const words: Record<LyricsSourceStatus['status'], string> = {
  synced: 'Timed',
  plain: 'Words only',
  none: 'Not found',
  failed: "Didn't answer",
}

export function LyricsSource({ song, lyrics, className = '' }: { song: QueueItem; lyrics: Lyrics | null; className?: string }) {
  const found = lyrics?.found
  const [open, setOpen] = useState(false)
  const none = lyrics !== null && !lyrics.lines.length && !lyrics.text.trim()
  // Nothing found: the way to look by another name is offered at once.
  if (!found && !none) return null
  return (
    <div className={`relative ${className}`}>
      <button onClick={() => setOpen((o) => !o)} aria-expanded={open} className="inline-flex items-center gap-1.5 rounded-full bg-ink/[0.07] px-3 py-1 text-[12px] text-ink/65 shadow-[inset_0_0_0_1px_var(--edge)] hover:bg-ink/[0.12] hover:text-ink">
        {found ? (
          <>
            Lyrics from <span className="font-medium text-ink/85">{found.label}</span>
            {found.choice.offset_ms ? <span className="tabular-nums text-ink/50">, {(found.choice.offset_ms / 1000).toFixed(2)} s</span> : null}
          </>
        ) : (
          'Look for the lyrics'
        )}
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden className={open ? 'rotate-180' : ''}>
          <path d="m6 9 6 6 6-6" />
        </svg>
      </button>
      <AnimatePresence>{open && <Picker key="picker" song={song} lyrics={lyrics} startSearching={none} onClose={() => setOpen(false)} />}</AnimatePresence>
    </div>
  )
}

function Picker({ song, lyrics, startSearching, onClose }: { song: QueueItem; lyrics: Lyrics | null; startSearching: boolean; onClose: () => void }) {
  useEscape(true, onClose)
  const ask = lyricsAsk(song)
  const found = lyrics?.found
  const choice = found?.choice
  const [statuses, setStatuses] = useState<LyricsSourceStatus[] | null>(null)
  const [searching, setSearching] = useState(startSearching || !!choice?.title)
  const [title, setTitle] = useState(choice?.title ?? song.title)
  const [artist, setArtist] = useState((choice?.artists ?? ask.artists).join(', '))
  const [offset, setOffset] = useState(choice?.offset_ms ?? 0)
  const box = useRef<HTMLDivElement>(null)

  const check = () => {
    setStatuses(null)
    lyricsCheck(ask)
      .then(setStatuses)
      .catch(() => setStatuses([]))
  }
  useEffect(check, [ask.key]) // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    const away = (e: MouseEvent) => !box.current?.contains(e.target as Node) && onClose()
    window.addEventListener('mousedown', away)
    return () => window.removeEventListener('mousedown', away)
  }, [onClose])

  const choose = async (source: string | null) => {
    await lyricsChoose(ask.key, source)
    lyricsChoiceChanged(ask.key)
  }
  // The offset is saved a moment after the last step, so a run of steps is one save.
  const saving = useRef(0)
  const nudge = (to: number) => {
    const v = Math.max(-15_000, Math.min(15_000, to))
    setOffset(v)
    window.clearTimeout(saving.current)
    saving.current = window.setTimeout(() => void lyricsOffset(ask.key, v).then(() => lyricsChoiceChanged(ask.key)), 400)
  }
  const search = async () => {
    const artists = artist.split(',').map((a) => a.trim()).filter(Boolean)
    await lyricsChoose(ask.key, null, { title: title.trim(), artists })
    lyricsChoiceChanged(ask.key)
    check()
  }
  const reset = async () => {
    await lyricsChoose(ask.key, null, { title: '', artists: [] })
    setTitle(song.title)
    setArtist(ask.artists.join(', '))
    lyricsChoiceChanged(ask.key)
    check()
  }

  const row = (on: boolean, label: string, note: string, run: () => void, disabled = false) => (
    <button
      key={label}
      role="menuitemradio"
      aria-checked={on}
      disabled={disabled}
      onClick={run}
      className="flex w-full items-center gap-3 rounded-[12px] px-3 py-2 text-left hover:bg-ink/[0.07] disabled:opacity-45 disabled:hover:bg-transparent"
    >
      <span aria-hidden className={`grid h-4 w-4 shrink-0 place-items-center rounded-full border-[1.5px] ${on ? 'border-accent' : 'border-ink/35'}`}>
        {on && <span className="h-2 w-2 rounded-full bg-accent" />}
      </span>
      <span className="min-w-0 flex-1 truncate text-[13px]">{label}</span>
      <span className="shrink-0 text-[11.5px] text-ink/50">{note}</span>
    </button>
  )

  return (
    <motion.div
      ref={box}
      role="dialog"
      aria-label="Lyrics source"
      initial={{ opacity: 0, y: 8, scale: 0.98 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: 6, scale: 0.98 }}
      transition={spring}
      className="popover absolute bottom-full left-0 z-30 mb-2 w-[340px] rounded-[18px] p-2"
    >
      <p className="px-3 pb-1 pt-2 text-[12px] text-ink/55">Just for this song. The order of the sources is in Settings.</p>
      <div role="menu" className="grid">
        {row(!choice?.source, 'Automatic', found && !choice?.source ? `Showing ${found.label}` : 'The first in your order', () => void choose(null))}
        {statuses === null ? (
          <p className="px-3 py-2 text-[12.5px] text-ink/50">Asking every source…</p>
        ) : (
          statuses.map((s) =>
            row(
              choice?.source === s.source,
              s.label,
              `${words[s.status]}${s.on ? '' : ', off'}`,
              () => void choose(s.source),
              s.status === 'none' || s.status === 'failed',
            ),
          )
        )}
      </div>
      <div className="mt-2 border-t border-edge px-3 pb-2 pt-3">
        <div className="flex items-center gap-2">
          <span className="flex-1 text-[12.5px]">Timing</span>
          <button onClick={() => nudge(offset - STEP)} aria-label="Earlier" className="grid h-7 w-7 place-items-center rounded-full bg-ink/[0.07] hover:bg-ink/[0.14]">
            −
          </button>
          <span className="w-[64px] text-center text-[12.5px] tabular-nums">{offset > 0 ? '+' : ''}{(offset / 1000).toFixed(2)} s</span>
          <button onClick={() => nudge(offset + STEP)} aria-label="Later" className="grid h-7 w-7 place-items-center rounded-full bg-ink/[0.07] hover:bg-ink/[0.14]">
            +
          </button>
          {offset !== 0 && (
            <button onClick={() => nudge(0)} className="rounded-full px-2 py-0.5 text-[12px] text-ink/60 hover:bg-ink/[0.08]">
              Reset
            </button>
          )}
        </div>
        <p className="mt-1 text-[11.5px] text-ink/50">Lyrics ahead of the singer? Move them later.</p>
      </div>
      <div className="border-t border-edge px-3 pb-2 pt-3">
        {!searching ? (
          <button onClick={() => setSearching(true)} className="text-[12.5px] text-accent-ink hover:underline">
            Wrong song? Search by title
          </button>
        ) : (
          <form
            onSubmit={(e) => {
              e.preventDefault()
              void search()
            }}
            className="grid gap-2"
          >
            <input value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Title" aria-label="Title to look for" className="field h-8 rounded-[10px] px-3 text-[12.5px]" />
            <input value={artist} onChange={(e) => setArtist(e.target.value)} placeholder="Artists, with commas" aria-label="Artists to look for" className="field h-8 rounded-[10px] px-3 text-[12.5px]" />
            <div className="flex items-center gap-2">
              <button type="submit" disabled={!title.trim()} className="btn-primary btn-sm">
                Look again
              </button>
              {choice?.title && (
                <button type="button" onClick={() => void reset()} className="btn-ghost btn-sm">
                  Use the song's own name
                </button>
              )}
            </div>
          </form>
        )}
      </div>
    </motion.div>
  )
}
