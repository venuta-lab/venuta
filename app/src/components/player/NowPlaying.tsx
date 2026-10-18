import { useEscape } from '../../lib/layers'
import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useCovers } from '../../lib/covers'
import { lineAt, useLyrics, useSongLyrics, type Lyrics } from '../../lib/lyrics'
import { LyricsSource } from '../lyrics/LyricsSource'
import { SyncMaker } from '../lyrics/SyncMaker'
import { confirmAsk } from '../Confirm'
import { ensureCatalog, trackAt } from '../../lib/catalog'
import { LyricsDock, SongActions } from './NowPlayingActions'
import { availabilityOf, songLength, current, isOnline, jumpTo, play, remoteArtUrl, seek, usePlayer } from '../../lib/player'
import { IconClose } from '../Icons'
import { artUrl, OnlineCover, RemoteArt } from '../RemoteArt'
import { SteadyArt } from '../SteadyArt'
import { AvailabilityBadge } from '../AvailabilityBadge'
import { SignalPath } from './NowPlayingBar'
import { Segmented } from '../ui'
import { SoundPanel } from './SoundPanel'
import { Stars } from './Stars'
import { PlayingFrom } from './PlayingFrom'
import { QueueRadio } from './QueueRadio'

const spring = { type: 'spring', stiffness: 300, damping: 32 } as const

/** Synced lyrics follow the song: the line being sung is bright, a click jumps to a line. */
function LyricsView(props: { lyrics: Lyrics | null; position: number; onTime: () => void }) {
  const { lyrics, position } = props
  const box = useRef<HTMLDivElement>(null)
  const userScrolled = useRef(0)
  const at = useMemo(() => lineAt(lyrics, position), [lyrics, position])
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
    return (
      <div className="grid justify-items-center gap-3 py-10 text-center">
        <p className="max-w-[46ch] text-[13px] text-ink/55">
          No lyrics for this song yet. Looking up details online finds synced lyrics for many songs, or paste the words and tap along to time them.
        </p>
        <button onClick={props.onTime} className="btn btn-sm">
          Paste lyrics
        </button>
      </div>
    )
  if (!lyrics.lines.length)
    return (
      <div className="thin-scroll h-full overflow-y-auto">
        <div className="sticky top-0 z-10 flex items-center gap-3 rounded-2xl bg-ink/[0.06] px-4 py-2.5 text-[13px] backdrop-blur-md">
          <span className="flex-1 text-ink/70">These words have no timing, so they can't follow the song.</span>
          <button onClick={props.onTime} className="btn btn-sm btn-primary">
            Time them
          </button>
        </div>
        <div className="whitespace-pre-wrap py-6 text-[16px] leading-relaxed text-ink/80">{lyrics.text}</div>
      </div>
    )
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
  if (!rest.length)
    return (
      <div className="py-6">
        <QueueRadio className="px-2" />
        <p className="py-8 text-center text-[13px] text-ink/55">Nothing after this song.</p>
      </div>
    )
  return (
    <>
    <QueueRadio className="px-2 pt-2" />
    <ol className="grid gap-1 py-2">
      {rest.map((q, k) => {
        const online = isOnline(q)
        const art = online ? null : cover(q.dir)
        const remote = remoteArtUrl(q)
        return (
          <li key={`${q.path || remote || q.title}#${k}`}>
            <button onClick={() => void jumpTo(s.index + 1 + k)} title="Play" className="flex w-full items-center gap-3 rounded-xl px-2 py-1.5 text-left transition-colors hover:bg-ink/[0.07] active:bg-ink/[0.1]">
              <span className="grid h-10 w-10 shrink-0 place-items-center overflow-hidden rounded-lg bg-ink/[0.08] text-ink/30">
                {online ? (
                  <RemoteArt url={remote} seed={q.album || q.title} size={40} imgClassName="h-full w-full object-cover" />
                ) : art ? (
                  <img src={art.src} alt="" className="h-full w-full object-cover" draggable={false} />
                ) : null}
              </span>
              <span className="min-w-0 flex-1">
                <span className="flex min-w-0 items-center gap-1.5">
                  <span className="truncate text-[13.5px]">{q.title}</span>
                  {online && <AvailabilityBadge availability={availabilityOf(q)} />}
                </span>
                <span className="block truncate text-[12px] text-ink/55">{q.artists.join(', ') || q.album_artist}</span>
              </span>
              <span className="text-[12px] tabular-nums text-ink/45">{songLength(q.duration_ms)}</span>
            </button>
          </li>
        )
      })}
    </ol>
    </>
  )
}

type Panel = 'lyrics' | 'next' | 'signal' | 'sound'

/** The song, large: cover, words, what's next and how it reaches you. */
export function NowPlaying(props: {
  open: boolean
  root: string
  panel: Panel
  onPanel: (p: Panel) => void
  onClose: () => void
  onAlbum: (dir: string) => void
  onArtist: (name: string) => void
  onOnlineArtist: (artist: { artist_id: string; name: string; artwork_url?: string | null }) => void
}) {
  const s = usePlayer()
  const song = current(s)
  const cover = useCovers(320)
  const art = song && !isOnline(song) ? cover(song.dir) : undefined
  const onlineArt = remoteArtUrl(song)
  // Online: LRCLIB by title/artist (display only). Local: file-backed lyrics as always.
  const remote = !!song && isOnline(song)
  // Your file's words when it has them, else the lyrics services' (online songs, and files
  // without lyrics: shown, never written to the file).
  const lyrics = useSongLyrics(props.root, song, props.open)
  const foundOnline = !!lyrics?.found
  // The song whose lyrics are being timed. It stays its song if another one starts: the
  // maker keeps its taps and offers the way back.
  const [timing, setTiming] = useState<string | null>(null)
  const timingNow = !!timing && props.panel === 'lyrics'
  const makerLyrics = useLyrics(props.root, timing ?? undefined)
  const unsaved = useRef(false)
  useEffect(() => {
    const on = (e: Event) => setTiming((e as CustomEvent<{ path: string }>).detail.path)
    window.addEventListener('venuta:time-lyrics', on)
    return () => window.removeEventListener('venuta:time-lyrics', on)
  }, [])
  const stopTiming = () => {
    unsaved.current = false
    setTiming(null)
  }
  /** Leaving the maker with taps not saved asks first; then `go`. */
  const leaving = async (go: () => void) => {
    if (timing && unsaved.current) {
      const ok = await confirmAsk({
        title: 'Leave without saving the timing?',
        text: 'The lines you tapped will be lost.',
        confirm: 'Leave',
        danger: true,
      })
      if (!ok) return
      stopTiming()
    }
    go()
  }
  const { open, onClose } = props
  useEscape(open, () => void leaving(onClose))
  // The timing button: the maker on the lyrics, or back to them.
  const onTiming = () => {
    if (!song || remote) return
    if (timingNow) void leaving(stopTiming)
    else {
      setTiming(song.path)
      props.onPanel('lyrics')
    }
  }
  const backTo = (path: string) =>
    void ensureCatalog(props.root).then(() => {
      const t = trackAt(path)
      if (t) play(props.root, [t], 0)
    })
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
            onClick={() => void leaving(onClose)}
            className="absolute right-6 top-5 z-10 grid h-9 w-9 place-items-center rounded-full bg-ink/[0.08] text-ink/70 hover:bg-ink/[0.15] hover:text-ink"
          >
            <IconClose />
          </button>

          <div className="relative grid h-full grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-12 px-14 pb-8 pt-14">
            <div className="flex min-h-0 min-w-0 flex-col items-center justify-center overflow-hidden">
              <motion.div
                layoutId="now-playing-cover"
                transition={spring}
                className="grid aspect-square w-full max-w-[min(400px,40vh)] place-items-center overflow-hidden rounded-[28px] bg-ink/[0.08] text-[48px] text-ink/25 shadow-[0_40px_90px_-30px_rgba(0,0,0,0.6)]"
              >
                {/* Held steady across a change of song (no blank frame). */}
                <SteadyArt src={onlineArt ? artUrl(onlineArt, 400) : art?.src} fallback={isOnline(song) ? <OnlineCover seed={song.album || song.title} /> : '♪'} />
              </motion.div>
              <div className="mt-8 w-full max-w-[min(400px,40vh)] text-center">
                <h2 className="title flex items-center justify-center gap-2 truncate text-[26px]">
                  <span className="truncate">{song.title}</span>
                  {isOnline(song) && <AvailabilityBadge availability={availabilityOf(song)} />}
                </h2>
                {s.buffering && (
                  <p className="mt-1 inline-flex items-center gap-1.5 text-[12.5px] text-ink/55">
                    <span className="spin h-2.5 w-2.5 rounded-full border-2 border-ink/20 border-t-ink/55" aria-hidden />
                    Opening…
                  </p>
                )}
                <SongActions
                  root={props.root}
                  song={song}
                  lyrics={lyrics}
                  onAlbum={(dir) => void leaving(() => props.onAlbum(dir))}
                  onArtist={(name) => void leaving(() => props.onArtist(name))}
                  onOnlineArtist={(artist) => void leaving(() => props.onOnlineArtist(artist))}
                  onTiming={onTiming}
                  stars={<Stars path={song.path} stars={song.stars} size={18} />}
                  from={<PlayingFrom className="mt-1 block truncate text-[12.5px] text-ink/45" onOpen={props.onClose} />}
                />
              </div>
            </div>

            <div className="flex min-h-0 min-w-0 flex-col">
              <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
                <Segmented
                  tabs
                  label="Now Playing"
                  value={props.panel}
                  onChange={(p) => (p === 'lyrics' ? props.onPanel(p) : void leaving(() => props.onPanel(p)))}
                  options={[
                    { value: 'lyrics', label: 'Lyrics' },
                    { value: 'next', label: 'Queue' },
                    { value: 'sound', label: 'Sound' },
                    { value: 'signal', label: 'Signal' },
                  ]}
                />
                <AnimatePresence>
                  {props.panel === 'lyrics' && (
                    <LyricsDock key="dock" root={props.root} song={song} lyrics={lyrics} timing={timingNow} onTiming={onTiming} canTime={!remote} />
                  )}
                </AnimatePresence>
              </div>
              {/* Under the bar above, so its popovers and names open over the words. */}
              <div className={`${timingNow ? '' : 'fade-y '}relative z-0 mt-3 min-h-0 flex-1 overflow-hidden`}>
                {props.panel === 'lyrics' &&
                  (timingNow && timing && makerLyrics ? (
                    <SyncMaker
                      key={timing}
                      root={props.root}
                      path={timing}
                      lyrics={makerLyrics}
                      current={song.path === timing}
                      title={trackAt(timing)?.title ?? timing.split('/').pop() ?? ''}
                      onBack={() => backTo(timing)}
                      onDirty={(d) => (unsaved.current = d)}
                      onClose={stopTiming}
                    />
                  ) : remote || foundOnline ? (
                    lyrics && (lyrics.lines.length > 0 || lyrics.text) ? (
                      <LyricsView lyrics={lyrics} position={s.position_ms} onTime={() => {}} />
                    ) : lyrics === null ? (
                      <p className="py-10 text-center text-[13px] text-ink/55">Looking up lyrics…</p>
                    ) : (
                      <p className="py-10 text-center text-[13px] leading-relaxed text-ink/55">
                        {lyrics.found?.instrumental ? 'An instrumental: no words to show.' : 'No lyrics found for this song. Look again by another name below.'}
                      </p>
                    )
                  ) : (
                    <LyricsView lyrics={lyrics} position={s.position_ms} onTime={() => setTiming(song.path)} />
                  ))}
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
              {/* Lyrics found online: where from, and the way to choose another for this song. */}
              {props.panel === 'lyrics' && !timingNow && (remote || foundOnline || (lyrics !== null && !lyrics.lines.length && !lyrics.text.trim())) && (
                <LyricsSource key={song.path || song.title} song={song} lyrics={lyrics} className="mt-2 shrink-0" />
              )}
            </div>
          </div>
        </motion.section>
      )}
    </AnimatePresence>
  )
}
