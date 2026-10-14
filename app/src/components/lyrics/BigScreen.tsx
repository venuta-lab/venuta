import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useRef, useState } from 'react'
import { loadCovers, useCovers } from '../../lib/covers'
import { closeBigScreen } from '../../lib/lyricWindows'
import { scaleOf, setLyricLook, useLyricLook } from '../../lib/lyricLook'
import { clock, connectPlayer, isOnline, toggle, usePlayerValue } from '../../lib/player'
import { cleanArtUrl } from '../RemoteArt'
import { LyricText } from './LyricText'
import { LookPanel } from './LookPanel'
import { IconClose, IconLyricStyle } from '../Icons'
import { hasIndic, latinOf, useLyricColour, useNowLyrics } from './useNowLyrics'

/** How long the pointer and the controls stay after the mouse stops. */
const IDLE_MS = 2500

/**
 * Lyrics filling a whole screen, for a TV or a second monitor, readable from the sofa: the line
 * being sung large, the next one coming, the song and where it is small at the bottom.
 */
export function BigScreen() {
  useEffect(() => connectPlayer(), [])
  const { song, root, lyrics, position, at, progress } = useNowLyrics()
  const look = useLyricLook()
  const colour = useLyricColour(look.colour, root, song?.dir)
  const duration = usePlayerValue((s) => s.duration_ms || s.queue[s.index]?.duration_ms || 0)
  const localDir = song && !isOnline(song) ? song.dir : ''
  const local = useCovers(640)(localDir)
  // An online song's picture comes from the catalog.
  const remoteSrc = song && isOnline(song) ? cleanArtUrl(song.artwork_url) : undefined
  const cover = remoteSrc ? { src: remoteSrc } : local
  useEffect(() => {
    if (root && localDir) void loadCovers(root, [localDir], 640)
  }, [root, localDir])
  const [awake, setAwake] = useState(true)
  const [styling, setStyling] = useState(false)
  const idle = useRef(0)
  // The style panel open keeps the controls (and the pointer) here.
  const shown = awake || styling

  useEffect(() => {
    const wake = () => {
      setAwake(true)
      window.clearTimeout(idle.current)
      idle.current = window.setTimeout(() => setAwake(false), IDLE_MS)
    }
    const key = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        if (styling) setStyling(false)
        else void closeBigScreen()
      }
      else if (e.key.toLowerCase() === 't') setLyricLook({ latin: !look.latin })
      else if (e.key === ' ') {
        e.preventDefault()
        void toggle()
      } else return
      wake()
    }
    wake()
    window.addEventListener('mousemove', wake)
    window.addEventListener('keydown', key)
    return () => {
      window.removeEventListener('mousemove', wake)
      window.removeEventListener('keydown', key)
      window.clearTimeout(idle.current)
    }
  }, [look.latin, styling])

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
  const indic = hasIndic(lines)

  return (
    <div className="relative h-full w-full overflow-hidden bg-black text-white" style={{ cursor: shown ? 'default' : 'none' }}>
      {/* The cover, as light behind everything. */}
      <AnimatePresence>
        {cover && (
          <motion.img
            key={cover.src}
            src={cover.src}
            alt=""
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 1.2 }}
            className="absolute inset-0 h-full w-full scale-125 object-cover"
            style={{ filter: 'blur(70px) saturate(1.4) brightness(0.5)' }}
          />
        )}
      </AnimatePresence>
      <div className="absolute inset-0" style={{ background: 'radial-gradient(120% 90% at 50% 45%, transparent 30%, rgb(0 0 0 / 0.65))' }} />

      <div className="relative flex h-full flex-col items-center justify-center px-[7vw]">
        {timed ? (
          <div className="grid w-full max-w-[min(92vw,1600px)] justify-items-center gap-[3vh]">
            <div className="min-h-[1.2em] opacity-40" style={{ fontSize: px * 0.55 }}>
              <AnimatePresence mode="popLayout" initial={false}>
                {prev !== undefined && (
                  <motion.div key={`p${at}`} initial={{ opacity: 0, y: 30 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -30 }} transition={{ duration: 0.45 }}>
                    <LyricText text={prev} look={look.look === 'karaoke' ? 'clean' : look.look} now={false} colour={colour} px={px * 0.55} />
                  </motion.div>
                )}
              </AnimatePresence>
            </div>
            <AnimatePresence mode="popLayout" initial={false}>
              <motion.div key={`n${at}`} initial={{ opacity: 0, y: 50, scale: 0.96 }} animate={{ opacity: 1, y: 0, scale: 1 }} exit={{ opacity: 0, y: -50 }} transition={{ type: 'spring', stiffness: 170, damping: 24 }}>
                <LyricText text={at < 0 ? '' : now} latin={latinOf(now, look.latin)} look={look.look} now progress={progress} colour={colour} px={px} />
              </motion.div>
            </AnimatePresence>
            <div className="min-h-[1.2em]">
              <AnimatePresence mode="popLayout" initial={false}>
                {next !== undefined && (
                  <motion.div key={`x${at}`} initial={{ opacity: 0, y: 40 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -40 }} transition={{ duration: 0.45 }}>
                    <LyricText text={next} latin={latinOf(next, look.latin)} look={look.look === 'karaoke' ? 'clean' : look.look} now={false} colour={colour} px={px * 0.62} />
                  </motion.div>
                )}
              </AnimatePresence>
            </div>
          </div>
        ) : (
          <div className="text-center">
            <div className="font-semibold tracking-[-0.02em]" style={{ fontSize: px * 0.9 }}>
              {song?.title ?? 'Nothing playing'}
            </div>
            <div className="mt-3 text-white/60" style={{ fontSize: px * 0.4 }}>
              {song ? song.artists.join(', ') || song.album_artist : 'Play a song in Venuta.'}
            </div>
            {song && (
              <div className="mt-8 text-white/45" style={{ fontSize: Math.max(16, px * 0.24) }}>
                {lyrics?.text ? 'These words have no times yet: time them in Venuta, and they follow the song here.' : lyrics ? 'No lyrics for this song.' : ''}
              </div>
            )}
          </div>
        )}
      </div>

      {/* The song, small, at the bottom. */}
      {song && (
        <div className="absolute inset-x-[5vw] bottom-[4vh] flex items-center gap-4 text-white/80">
          {cover && <img src={cover.src} alt="" className="h-[6vh] w-[6vh] rounded-lg object-cover shadow-lg" />}
          <div className="min-w-0 flex-1">
            <div className="truncate font-semibold" style={{ fontSize: Math.max(15, view.h * 0.022) }}>
              {song.title}
            </div>
            <div className="truncate text-white/55" style={{ fontSize: Math.max(13, view.h * 0.018) }}>
              {song.artists.join(', ') || song.album_artist}
            </div>
            <div className="mt-2 flex items-center gap-3 text-white/50" style={{ fontSize: Math.max(12, view.h * 0.015) }}>
              <span className="tabular-nums">{clock(position)}</span>
              <div className="h-[3px] flex-1 overflow-hidden rounded-full bg-white/15">
                <div className="h-full rounded-full" style={{ width: `${duration ? Math.min(100, (position / duration) * 100) : 0}%`, background: colour }} />
              </div>
              <span className="tabular-nums">{clock(duration)}</span>
            </div>
          </div>
        </div>
      )}

      {/* Controls, only while the mouse moves: two quiet buttons in the corner. */}
      <AnimatePresence>
        {shown && (
          <motion.div
            initial={{ opacity: 0, y: -8 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -8 }}
            className="absolute right-[3vw] top-[3vh] flex flex-col items-end gap-3"
          >
            <div className="flex items-center gap-1 rounded-full bg-black/45 p-1 shadow-[0_10px_30px_-12px_rgb(0_0_0/0.8),inset_0_0_0_1px_rgb(255_255_255/0.1)] backdrop-blur-xl">
              <button
                onClick={() => setStyling((s) => !s)}
                aria-expanded={styling}
                className={`flex h-10 items-center gap-2 rounded-full pl-3 pr-4 text-[14px] font-medium transition-colors ${styling ? 'bg-white text-black' : 'text-white/85 hover:bg-white/15 hover:text-white'}`}
              >
                <IconLyricStyle width={19} height={19} />
                Style
              </button>
              <button
                onClick={() => void closeBigScreen()}
                aria-label="Close the big screen"
                title="Close (Esc)"
                className="grid h-10 w-10 place-items-center rounded-full text-white/80 hover:bg-white/15 hover:text-white"
              >
                <IconClose />
              </button>
            </div>
            <AnimatePresence>
              {styling && (
                <motion.div
                  initial={{ opacity: 0, y: -6, scale: 0.97 }}
                  animate={{ opacity: 1, y: 0, scale: 1 }}
                  exit={{ opacity: 0, y: -6, scale: 0.97 }}
                  transition={{ type: 'spring', stiffness: 420, damping: 32 }}
                  style={{ transformOrigin: 'top right' }}
                  className="w-[360px] rounded-[24px] bg-black/60 p-4 shadow-[0_24px_60px_-20px_rgb(0_0_0/0.9),inset_0_0_0_1px_rgb(255_255_255/0.1)] backdrop-blur-2xl"
                >
                  <LookPanel tone="dark" coverColour={colour} indic={indic} />
                </motion.div>
              )}
            </AnimatePresence>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  )
}
