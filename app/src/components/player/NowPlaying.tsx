import { useEscape } from '../../lib/layers'
import { invoke } from '@tauri-apps/api/core'
import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useCovers } from '../../lib/covers'
import { inApp } from '../../lib/engine'
import { clock, current, jumpTo, seek, usePlayer } from '../../lib/player'
import { IconClose } from '../Icons'
import { SignalPath } from './NowPlayingBar'
import { Segmented } from '../ui'
import { SoundPanel } from './SoundPanel'
import { Stars } from './Stars'

const spring = { type: 'spring', stiffness: 300, damping: 32 } as const

export interface Lyrics {
  lines: [number, string][]
  text: string
}

export function useLyrics(root: string, path: string | undefined) {
  const [lyrics, setLyrics] = useState<{ path: string; value: Lyrics | null }>({ path: '', value: null })
  useEffect(() => {
    if (!path || !inApp()) return
    let live = true
    invoke<Lyrics>('song_lyrics', { root, path })
      .then((value) => live && setLyrics({ path, value }))
      .catch(() => live && setLyrics({ path, value: { lines: [], text: '' } }))
    return () => {
      live = false
    }
  }, [root, path])
  return lyrics.path === path ? lyrics.value : null
}

/** The line being sung at `position` (a moment early, so it's read as it's sung); -1 before the first. */
export function lineAt(lyrics: Lyrics | null, position: number): number {
  if (!lyrics?.lines.length) return -1
  let lo = 0
  let hi = lyrics.lines.length - 1
  let found = -1
  const t = position + 250
  while (lo <= hi) {
    const mid = (lo + hi) >> 1
    if (lyrics.lines[mid][0] <= t) {
      found = mid
      lo = mid + 1
    } else hi = mid - 1
  }
  return found
}

/** Synced lyrics follow the song: the line being sung is bright, a click jumps to a line. */
function LyricsView(props: { lyrics: Lyrics | null; position: number }) {
  const { lyrics, position } = props
  const box = useRef<HTMLDivElement>(null)
  const userScrolled = useRef(0)
  const at = useMemo(() => {
    if (!lyrics?.lines.length) return -1
    let lo = 0
    let hi = lyrics.lines.length - 1
    let found = -1
    // Lines light up a moment early, so they're read as they're sung.
    const t = position + 250
    while (lo <= hi) {
      const mid = (lo + hi) >> 1
      if (lyrics.lines[mid][0] <= t) {
        found = mid
        lo = mid + 1
      } else hi = mid - 1
    }
    return found
  }, [lyrics, position])
  useEffect(() => {
    if (at < 0 || Date.now() - userScrolled.current < 4000) return
    // Scroll only the lyrics (scrollIntoView would move every scrollable parent too).
    const el = box.current?.querySelector<HTMLElement>(`[data-line="${at}"]`)
    if (el && box.current) {
      box.current.scrollTo({ top: el.offsetTop - box.current.clientHeight / 2 + el.offsetHeight / 2, behavior: 'smooth' })
    }
  }, [at])

  if (!lyrics) return <p className="py-10 text-center text-[13px] text-ink/55">Finding the words…</p>
  if (!lyrics.lines.length && !lyrics.text)
    return <p className="py-10 text-center text-[13px] text-ink/55">No lyrics for this song yet. Looking up details online finds synced lyrics for many songs.</p>
  if (!lyrics.lines.length)
    return <div className="whitespace-pre-wrap py-6 text-[16px] leading-relaxed text-ink/80">{lyrics.text}</div>
  return (
    <div
      ref={box}
      onWheel={() => (userScrolled.current = Date.now())}
      className="no-scrollbar relative h-full overflow-y-auto py-[30vh]"
      style={{ maskImage: 'linear-gradient(transparent, black 18%, black 82%, transparent)' }}
    >
      {lyrics.lines.map(([ms, line], i) => (
        <button
          key={i}
          data-line={i}
          onClick={() => void seek(ms)}
          className={`block w-full origin-left py-1.5 text-left text-[22px] font-semibold leading-snug tracking-[-0.01em] transition-[color,transform,opacity] duration-300 ${
            i === at ? 'scale-[1.02] text-ink' : i < at ? 'text-ink/35 hover:text-ink/60' : 'text-ink/45 hover:text-ink/70'
          }`}
        >
          {line || '♪'}
        </button>
      ))}
    </div>
  )
}

function UpNext() {
  const s = usePlayer()
  const cover = useCovers(320)
  const rest = s.queue.slice(s.index + 1, s.index + 40)
  if (!rest.length) return <p className="py-10 text-center text-[13px] text-ink/55">Nothing after this song.</p>
  return (
    <ol className="grid gap-1 py-2">
      {rest.map((q, k) => {
        const art = cover(q.dir)
        return (
          <li key={`${q.path}#${k}`}>
            <button onClick={() => void jumpTo(s.index + 1 + k)} title="Play" className="flex w-full items-center gap-3 rounded-xl px-2 py-1.5 text-left transition-colors hover:bg-ink/[0.07] active:bg-ink/[0.1]">
              <span className="h-10 w-10 shrink-0 overflow-hidden rounded-lg bg-ink/[0.08]">
                {art && <img src={art.src} alt="" className="h-full w-full object-cover" draggable={false} />}
              </span>
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[13.5px]">{q.title}</span>
                <span className="block truncate text-[12px] text-ink/55">{q.artists.join(', ') || q.album_artist}</span>
              </span>
              <span className="text-[12px] tabular-nums text-ink/45">{clock(q.duration_ms)}</span>
            </button>
          </li>
        )
      })}
    </ol>
  )
}

type Panel = 'lyrics' | 'next' | 'signal' | 'sound'

/** The song, large: cover, words, what's next and how it reaches you. */
export function NowPlaying(props: { open: boolean; root: string; panel: Panel; onPanel: (p: Panel) => void; onClose: () => void; onAlbum: (dir: string) => void }) {
  const s = usePlayer()
  const song = current(s)
  const cover = useCovers(320)
  const art = song ? cover(song.dir) : undefined
  const lyrics = useLyrics(props.root, props.open ? song?.path : undefined)
  const { open, onClose } = props
  useEscape(open, onClose)
  return (
    <AnimatePresence>
      {open && song && (
        <motion.section
          aria-label="Now Playing"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.25 }}
          className="absolute inset-0 z-30 overflow-hidden"
        >
          {/* No background of its own: the window's backdrop turns to this song's cover, so the
              title bar, the rail and the player all share one light. */}

          <button
            aria-label="Close Now Playing (Esc)"
            onClick={onClose}
            className="absolute right-6 top-5 z-10 grid h-9 w-9 place-items-center rounded-full bg-ink/[0.08] text-ink/70 hover:bg-ink/[0.15] hover:text-ink"
          >
            <IconClose />
          </button>

          <div className="relative grid h-full grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-12 px-14 pb-8 pt-14">
            <div className="flex min-h-0 min-w-0 flex-col items-center justify-center overflow-hidden">
              <motion.div
                layoutId="now-playing-cover"
                transition={spring}
                className="aspect-square w-full max-w-[min(400px,40vh)] overflow-hidden rounded-[28px] bg-ink/[0.08] shadow-[0_40px_90px_-30px_rgba(0,0,0,0.6)]"
              >
                {art && <img src={art.src} alt="" className="h-full w-full object-cover" draggable={false} />}
              </motion.div>
              <div className="mt-8 w-full max-w-[min(400px,40vh)] text-center">
                <h2 className="truncate text-[26px] font-semibold tracking-[-0.02em]">{song.title}</h2>
                <p className="mt-1 truncate text-[15px] text-ink/65">{song.artists.join(', ') || song.album_artist}</p>
                <button onClick={() => props.onAlbum(song.dir)} className="mt-0.5 max-w-full truncate text-[13px] text-ink/50 hover:text-ink hover:underline">
                  {song.album}
                </button>
                <div className="mt-3 flex justify-center">
                  <Stars path={song.path} stars={song.stars} size={18} />
                </div>
              </div>
            </div>

            <div className="flex min-h-0 min-w-0 flex-col">
              <Segmented
                tabs
                label="Now Playing"
                className="self-start"
                value={props.panel}
                onChange={props.onPanel}
                options={[
                  { value: 'lyrics', label: 'Lyrics' },
                  { value: 'next', label: 'Up next' },
                  { value: 'sound', label: 'Sound' },
                  { value: 'signal', label: 'Signal' },
                ]}
              />
              <div className="fade-y mt-3 min-h-0 flex-1 overflow-hidden">
                {props.panel === 'lyrics' && <LyricsView lyrics={lyrics} position={s.position_ms} />}
                {props.panel === 'next' && (
                  <div className="thin-scroll h-full overflow-y-auto pt-3">
                    <UpNext />
                  </div>
                )}
                {props.panel === 'sound' && (
                  <div className="thin-scroll h-full overflow-y-auto pr-1 pt-3">
                    <SoundPanel />
                  </div>
                )}
                {props.panel === 'signal' && (
                  <div className="glass mt-3 rounded-2xl p-5 text-[13px]">
                    <SignalPath signal={s.signal} />
                  </div>
                )}
              </div>
            </div>
          </div>
        </motion.section>
      )}
    </AnimatePresence>
  )
}
