import { currentMonitor, getCurrentWindow, PhysicalPosition, PhysicalSize } from '@tauri-apps/api/window'
import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { inApp } from '../../lib/engine'
import { grownStrip, stripRows } from '../../lib/lyricRows'
import { scaleOf, useLyricLook } from '../../lib/lyricLook'
import { connectPlayer } from '../../lib/player'
import { keyAction } from '../../lib/secondChoice'
import { rowViewsAt } from '../../lib/useSecondLines'
import { LyricText } from './LyricText'
import { BAND, LyricsWindow, type Frame } from './LyricsWindow'
import { NowLyricsProvider } from './NowLyrics'
import { useNowLyrics } from './useNowLyrics'

/** Paused this long (or nothing playing), the lines fade away; they come back with the music. */
const FADE_AFTER_MS = 4000

/**
 * The lines alone, floating over the desktop and every window, in a window of their own
 * (LyricsWindow: its background, place, size and lock).
 */
export function DesktopLyrics() {
  useEffect(() => connectPlayer(), [])
  return (
    <NowLyricsProvider>
      <LyricsWindow win="overlay">{(f) => <Lines frame={f} />}</LyricsWindow>
    </NowLyricsProvider>
  )
}

/**
 * The strip grows to fit the rows switched on: when the song's rows change (a switch, or a song
 * that has them), it is made taller or shorter by as much as keeps the words their size, along
 * the edge away from the screen's (lib/lyricRows.ts). Its size by hand stays its own.
 */
function useStripGrowth(rows: number) {
  const was = useRef(rows)
  useEffect(() => {
    const from = was.current
    was.current = rows
    if (from === rows || !inApp()) return
    const w = getCurrentWindow()
    void (async () => {
      const [mon, pos, size] = await Promise.all([currentMonitor(), w.outerPosition(), w.outerSize()])
      if (!mon) return
      const k = mon.scaleFactor
      const a = mon.workArea
      const g = grownStrip({ x: pos.x, y: pos.y, w: size.width, h: size.height }, BAND * k, from, rows, { x: a.position.x, y: a.position.y, w: a.size.width, h: a.size.height })
      if (g.h === size.height && g.y === pos.y) return
      await w.setSize(new PhysicalSize(g.w, g.h))
      await w.setPosition(new PhysicalPosition(g.x, g.y))
    })().catch(() => {})
  }, [rows])
}

function Lines({ frame }: { frame: Frame }) {
  const { song, playing, lyrics, seconds, position, at, progress } = useNowLyrics()
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
  // The sung line's pronunciation and translation: the same rows for every line of the song.
  const rows = seconds.rows
  const rowsKey = `${rows.roman}|${rows.translation}`
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
  const share = stripRows(look.lines, rows)
  useStripGrowth(share)
  // T and Shift+T while the strip is unlocked (being arranged, it hears the keys).
  const toggleRow = useRef(seconds.toggle)
  useEffect(() => {
    toggleRow.current = (kind) => seconds.offered[kind] && seconds.toggle(kind)
  }, [seconds])
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      const row = keyAction(e)
      if (row) toggleRow.current(row)
    }
    window.addEventListener('keydown', key)
    return () => window.removeEventListener('keydown', key)
  }, [])
  const fits = (area.h - 24) / share
  const want = Math.max(14, fits * 0.5 * scaleOf(look.size))
  const [fit, setFit] = useState(1)
  // A new line (or size, or window): measured afresh.
  const measureKey = `${at}|${want}|${area.w}|${look.lines}|${look.look}|${rowsKey}`
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
                <LyricText
                  text={now ?? ''}
                  seconds={rowViewsAt(seconds, at, { position, progress })}
                  lang={seconds.lang ?? undefined}
                  look={look.look}
                  now
                  progress={progress}
                  colour={frame.colour}
                  px={px}
                  ground={frame.ground}
                />
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
