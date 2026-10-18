import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useRef, useState } from 'react'
import { scaleOf, setLyricLook, useLyricLook } from '../../lib/lyricLook'
import { clock, songLength, connectPlayer, next as nextSong, previous, seek, toggle, usePlayerValue } from '../../lib/player'
import { LyricText } from './LyricText'
import { LyricsWindow, type Frame } from './LyricsWindow'
import { IconNext, IconPause, IconPlay, IconPrevious } from '../Icons'
import { latinOf, useNowLyrics, useWindowCover } from './useNowLyrics'

/**
 * Lyrics filling a whole screen (or a window), for a TV or a second monitor, readable from the
 * sofa: the line being sung large, the next one coming, the song and where it is small at the
 * bottom. Its window (LyricsWindow) gives it its background, place, size and lock; Clear shows
 * the words alone.
 */
export function BigScreen() {
  useEffect(() => connectPlayer(), [])
  return <LyricsWindow win="stage">{(f) => <Stage frame={f} />}</LyricsWindow>
}

function Stage({ frame }: { frame: Frame }) {
  const { song, lyrics, position, at, progress } = useNowLyrics()
  const look = useLyricLook('stage')
  const { colour, ground, clear, awake } = frame
  const duration = usePlayerValue((s) => s.duration_ms || s.queue[s.index]?.duration_ms || 0)
  const playing = usePlayerValue((s) => s.playing)
  const cover = useWindowCover()
  // For the arrow keys, which are set up once.
  const positionNow = useRef(0)
  useEffect(() => {
    positionNow.current = position
  }, [position])

  // T for Roman letters, Space to play or pause, the arrows to move through the song. (Esc and
  // F11 are the window's.)
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key.toLowerCase() === 't') setLyricLook('stage', { latin: !look.latin })
      else if (e.key === ' ') {
        e.preventDefault()
        void toggle()
      } else if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') void seek(positionNow.current + (e.key === 'ArrowRight' ? 5000 : -5000))
    }
    window.addEventListener('keydown', key)
    return () => window.removeEventListener('keydown', key)
  }, [look.latin])

  // Sized to the screen: a line reads from across a room.
  const [view, setView] = useState({ w: window.innerWidth, h: window.innerHeight })
  useEffect(() => {
    const r = () => setView({ w: window.innerWidth, h: window.innerHeight })
    window.addEventListener('resize', r)
    return () => window.removeEventListener('resize', r)
  }, [])
  const px = Math.min(view.w * 0.05, view.h * 0.085) * scaleOf(look.size)

  const lines = lyrics?.lines ?? []
  const timed = lines.length > 0
  const now = timed && at >= 0 ? lines[at][1] : ''
  const next = timed ? lines[at + 1]?.[1] : undefined
  const prev = timed && at > 0 ? lines[at - 1][1] : undefined
  const quiet = look.look === 'karaoke' ? 'clean' : look.look

  return (
    <div className="relative h-full w-full">
      <div className="relative flex h-full flex-col items-center justify-center px-[7vw]">
        {timed ? (
          <div className="grid w-full max-w-[min(92vw,1600px)] justify-items-center gap-[3vh]">
            <div className="min-h-[1.2em] opacity-40" style={{ fontSize: px * 0.55 }}>
              <AnimatePresence mode="popLayout" initial={false}>
                {prev !== undefined && (
                  <motion.div key={`p${at}`} initial={{ opacity: 0, y: 30 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -30 }} transition={{ duration: 0.45 }}>
                    <LyricText text={prev} look={quiet} now={false} colour={colour} px={px * 0.55} ground={ground} />
                  </motion.div>
                )}
              </AnimatePresence>
            </div>
            <AnimatePresence mode="popLayout" initial={false}>
              <motion.div key={`n${at}`} initial={{ opacity: 0, y: 50, scale: 0.96 }} animate={{ opacity: 1, y: 0, scale: 1 }} exit={{ opacity: 0, y: -50 }} transition={{ type: 'spring', stiffness: 170, damping: 24 }}>
                <LyricText text={at < 0 ? '' : now} latin={latinOf(now, look.latin)} look={look.look} now progress={progress} colour={colour} px={px} ground={ground} />
              </motion.div>
            </AnimatePresence>
            <div className="min-h-[1.2em]">
              <AnimatePresence mode="popLayout" initial={false}>
                {next !== undefined && (
                  <motion.div key={`x${at}`} initial={{ opacity: 0, y: 40 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -40 }} transition={{ duration: 0.45 }}>
                    <LyricText text={next} latin={latinOf(next, look.latin)} look={quiet} now={false} colour={colour} px={px * 0.62} ground={ground} />
                  </motion.div>
                )}
              </AnimatePresence>
            </div>
          </div>
        ) : clear ? null : (
          // Clear shows the words alone: without them, nothing.
          <div className="text-center">
            <div className="font-semibold tracking-[-0.02em]" style={{ fontSize: px * 0.9 }}>
              {song?.title ?? 'Nothing playing'}
            </div>
            <div className="mt-3 opacity-60" style={{ fontSize: px * 0.4 }}>
              {song ? song.artists.join(', ') || song.album_artist : 'Play a song in Venuta.'}
            </div>
            {song && (
              <div className="mt-8 opacity-45" style={{ fontSize: Math.max(16, px * 0.24) }}>
                {lyrics?.text ? 'These words have no times yet: time them in Venuta, and they follow the song here.' : lyrics ? 'No lyrics for this song.' : ''}
              </div>
            )}
          </div>
        )}
      </div>

      {/* The song, small, at the bottom (not in Clear: the words alone). */}
      {song && !clear && (
        <div className="absolute inset-x-[5vw] bottom-[4vh] flex items-center gap-4 opacity-80">
          {cover && <img src={cover.src} alt="" className="h-[6vh] w-[6vh] rounded-lg object-cover shadow-lg" />}
          <div className="min-w-0 flex-1">
            <div className="truncate font-semibold" style={{ fontSize: Math.max(15, view.h * 0.022) }}>
              {song.title}
            </div>
            <div className="truncate opacity-70" style={{ fontSize: Math.max(13, view.h * 0.018) }}>
              {song.artists.join(', ') || song.album_artist}
            </div>
            <div className="mt-2 flex items-center gap-3 opacity-60" style={{ fontSize: Math.max(12, view.h * 0.015) }}>
              <span className="tabular-nums">{clock(position)}</span>
              {/* A click puts the song there (while the controls show; the line stays thin). */}
              <div
                role="slider"
                aria-label="Position in the song"
                aria-valuemin={0}
                aria-valuemax={Math.round(duration / 1000)}
                aria-valuenow={Math.round(position / 1000)}
                onClick={(e) => {
                  const r = e.currentTarget.getBoundingClientRect()
                  if (duration) void seek(Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)) * duration)
                }}
                className={`flex flex-1 items-center py-2 ${awake ? 'cursor-pointer' : ''}`}
              >
                <div className={`w-full overflow-hidden rounded-full transition-[height] duration-200 ${ground === 'dark' ? 'bg-white/15' : 'bg-black/15'} ${awake ? 'h-[5px]' : 'h-[3px]'}`}>
                  <div className="h-full rounded-full" style={{ width: `${duration ? Math.min(100, (position / duration) * 100) : 0}%`, background: colour }} />
                </div>
              </div>
              <span className="tabular-nums">{songLength(duration)}</span>
            </div>
          </div>
          {/* Previous, play and next, only while the mouse moves (Space plays and pauses). */}
          <AnimatePresence>
            {awake && (
              <motion.div
                initial={{ opacity: 0, y: 8 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: 8 }}
                className="flex shrink-0 items-center gap-1 rounded-full bg-black/60 p-1 text-white shadow-[inset_0_0_0_1px_rgb(255_255_255/0.1)]"
              >
                <button onClick={() => void previous()} aria-label="Previous" className="grid h-14 w-14 place-items-center rounded-full text-white/80 hover:bg-white/15 hover:text-white">
                  <IconPrevious width={24} height={24} />
                </button>
                <button onClick={() => void toggle()} aria-label={playing ? 'Pause' : 'Play'} title="Space" className="grid h-16 w-16 place-items-center rounded-full bg-white text-black hover:scale-105 active:scale-95 transition-transform">
                  {playing ? <IconPause width={28} height={28} /> : <IconPlay width={28} height={28} />}
                </button>
                <button onClick={() => void nextSong()} aria-label="Next" className="grid h-14 w-14 place-items-center rounded-full text-white/80 hover:bg-white/15 hover:text-white">
                  <IconNext width={24} height={24} />
                </button>
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      )}
    </div>
  )
}
