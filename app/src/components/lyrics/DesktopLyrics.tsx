import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { scaleOf, useLyricLook } from '../../lib/lyricLook'
import { connectPlayer } from '../../lib/player'
import { LyricText } from './LyricText'
import { LyricsWindow, type Frame } from './LyricsWindow'
import { latinOf, useNowLyrics } from './useNowLyrics'

/** Paused this long (or nothing playing), the lines fade away; they come back with the music. */
const FADE_AFTER_MS = 4000

/**
 * The lines alone, floating over the desktop and every window, in a window of their own
 * (LyricsWindow: its background, place, size and lock).
 */
export function DesktopLyrics() {
  useEffect(() => connectPlayer(), [])
  return <LyricsWindow win="overlay">{(f) => <Lines frame={f} />}</LyricsWindow>
}

/** The rows a size is a share of: the line, the next under it, Roman letters too. */
function rowsOf(lines: 1 | 2, latin: boolean) {
  // Line height 1.18 for the line; the next is 0.7 of it with a little gap; Roman letters 0.52.
  return 1.18 + (lines === 2 ? 0.83 + 0.06 : 0) + (latin ? 0.83 : 0)
}

function Lines({ frame }: { frame: Frame }) {
  const { song, playing, lyrics, at, progress } = useNowLyrics()
  const look = useLyricLook('overlay')
  // Fade when nothing plays for a while (back as soon as it plays).
  const [faded, setFaded] = useState(false)
  useEffect(() => {
    if (playing) return
    const t = window.setTimeout(() => setFaded(true), FADE_AFTER_MS)
    return () => {
      window.clearTimeout(t)
      setFaded(false)
    }
  }, [playing])
  const quiet = faded && !playing

  const lines = lyrics?.lines ?? []
  const now = at >= 0 ? lines[at]?.[1] : undefined
  const next = lines[Math.max(0, at + 1)]?.[1]
  const latin = latinOf(now, look.latin)
  const shown = !!song && lines.length > 0 && !quiet
  const editing = !frame.locked

  // The words size to the window: the chosen size is a share of what fits in its height (Medium
  // keeps the size it always had in its first place), and a line that wraps shrinks only as much
  // as it must, so nothing is ever cut off.
  const box = useRef<HTMLDivElement>(null)
  const inner = useRef<HTMLDivElement>(null)
  // The room the words have (the window, less the bar while it shows).
  const [area, setArea] = useState({ w: window.innerWidth, h: window.innerHeight })
  useEffect(() => {
    const b = box.current
    if (!b) return
    const o = new ResizeObserver(() => setArea({ w: b.clientWidth, h: b.clientHeight }))
    o.observe(b)
    return () => o.disconnect()
  }, [])
  const fits = (area.h - 24) / rowsOf(look.lines, !!latin)
  const want = Math.max(14, fits * 0.5 * scaleOf(look.size))
  const [fit, setFit] = useState(1)
  // A new line (or size, or window): measured afresh.
  const measureKey = `${at}|${want}|${area.w}|${look.lines}|${look.look}|${!!latin}`
  const [measured, setMeasured] = useState(measureKey)
  if (measured !== measureKey) {
    setMeasured(measureKey)
    setFit(1)
  }
  useLayoutEffect(() => {
    const b = box.current
    const i = inner.current
    if (!b || !i) return
    // Its padding (p-3) is no room for words.
    const room = b.clientHeight - 24
    if (i.scrollHeight - room > 1 && fit > 0.4) setFit(Math.max(0.4, fit * (room / i.scrollHeight) * 0.98))
  }, [fit, measureKey])
  const px = want * fit

  return (
    <div ref={box} className="flex h-full w-full items-center justify-center overflow-hidden p-3">
      <motion.div ref={inner} animate={{ opacity: shown || editing ? 1 : 0 }} transition={{ duration: 0.6 }} className="w-full select-none">
        {editing && !shown ? (
          <LyricText text={song ? 'Lyrics show here as the song plays' : 'Play a song with lyrics'} look={look.look} now progress={1} colour={frame.colour} px={px * 0.7} ground={frame.ground} />
        ) : (
          <>
            <AnimatePresence mode="popLayout" initial={false}>
              <motion.div key={`n${at}`} initial={{ opacity: 0, y: 14 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -14 }} transition={{ duration: 0.3 }}>
                <LyricText text={now ?? ''} latin={latin} look={look.look} now progress={progress} colour={frame.colour} px={px} ground={frame.ground} />
              </motion.div>
            </AnimatePresence>
            {look.lines === 2 && next !== undefined && (
              <AnimatePresence mode="popLayout" initial={false}>
                <motion.div key={`x${at}`} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: 0.3 }} className="mt-1">
                  <LyricText text={next} look={look.look === 'karaoke' ? 'clean' : look.look} now={false} colour={frame.colour} px={px * 0.7} ground={frame.ground} />
                </motion.div>
              </AnimatePresence>
            )}
          </>
        )}
      </motion.div>
    </div>
  )
}
