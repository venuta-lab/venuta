import { getCurrentWindow } from '@tauri-apps/api/window'
import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useState } from 'react'
import { closeDesktopLyrics, lockDesktopLyrics, lockedSaved, saveBounds } from '../../lib/lyricWindows'
import { scaleOf, useLyricLook } from '../../lib/lyricLook'
import { connectPlayer } from '../../lib/player'
import { LyricText } from './LyricText'
import { LookStrip } from './LookPanel'
import { IconClose, IconLock } from '../Icons'
import { hasIndic, latinOf, useLyricColour, useNowLyrics } from './useNowLyrics'

/** Paused this long (or nothing playing), the lines fade away; they come back with the music. */
const FADE_AFTER_MS = 4000

/**
 * The lines alone, floating over the desktop and every window. Locked (as it usually is), clicks
 * go through it to whatever is under it; unlocked, drag it, size it, change its look.
 */
export function DesktopLyrics() {
  useEffect(() => connectPlayer(), [])
  const { song, root, playing, lyrics, at, progress } = useNowLyrics()
  const look = useLyricLook()
  const colour = useLyricColour(look.colour, root, song?.dir)
  const [locked, setLocked] = useState(lockedSaved())
  const [quiet, setQuiet] = useState(false)

  // Fade when nothing plays for a while.
  useEffect(() => {
    if (playing) {
      setQuiet(false)
      return
    }
    const t = window.setTimeout(() => setQuiet(true), FADE_AFTER_MS)
    return () => window.clearTimeout(t)
  }, [playing])

  // Where it is and how big, kept for next time.
  useEffect(() => {
    const w = getCurrentWindow()
    const keep = async () => {
      const [pos, size, scale] = await Promise.all([w.outerPosition(), w.outerSize(), w.scaleFactor()])
      saveBounds([pos.x / scale, pos.y / scale, size.width / scale, size.height / scale])
    }
    const off = [w.onMoved(() => void keep()), w.onResized(() => void keep())]
    return () => {
      off.forEach((p) => void p.then((un) => un()))
    }
  }, [])

  // Locked from the tray or the shortcut: follow it here too.
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key === 'desktop-lyrics-locked') setLocked(lockedSaved())
    }
    window.addEventListener('storage', onStorage)
    return () => window.removeEventListener('storage', onStorage)
  }, [])

  const lines = lyrics?.lines ?? []
  const now = at >= 0 ? lines[at]?.[1] : undefined
  const next = lines[Math.max(0, at + 1)]?.[1]
  const shown = !!song && lines.length > 0 && !quiet
  const px = 34 * scaleOf(look.size)
  const editing = !locked

  async function lock(on: boolean) {
    setLocked(on)
    await lockDesktopLyrics(on)
  }

  return (
    <div
      className="flex h-full w-full flex-col items-center justify-center overflow-hidden p-3"
      onMouseDown={(e) => {
        // Unlocked: drag it anywhere but the controls.
        if (editing && e.button === 0 && !(e.target as HTMLElement).closest('button,select,[role=radiogroup]')) void getCurrentWindow().startDragging()
      }}
      style={{
        borderRadius: 16,
        outline: editing ? '2px dashed rgb(255 255 255 / 0.55)' : 'none',
        outlineOffset: -4,
        background: editing ? 'rgb(0 0 0 / 0.28)' : 'transparent',
        cursor: editing ? 'move' : 'default',
      }}
    >
      <AnimatePresence>
        {editing && (
          <motion.div
            initial={{ opacity: 0, y: -6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }}
            className="mb-2 flex max-w-full items-center gap-1 rounded-full bg-black/65 p-1 shadow-[0_10px_30px_-10px_rgb(0_0_0/0.8),inset_0_0_0_1px_rgb(255_255_255/0.1)] backdrop-blur-xl"
          >
            <button
              onClick={() => void lock(true)}
              className="flex h-7 items-center gap-1.5 rounded-full bg-white pl-2.5 pr-3 text-[12.5px] font-semibold text-black transition-transform active:scale-95"
              title="Clicks go through it again (move it from the tray)"
            >
              <IconLock width={14} height={14} strokeWidth={2.2} />
              Lock in place
            </button>
            <span className="mx-1 h-4 w-px bg-white/15" aria-hidden />
            <LookStrip coverColour={colour} indic={hasIndic(lines)} />
            <span className="mx-1 h-4 w-px bg-white/15" aria-hidden />
            <button
              onClick={() => void closeDesktopLyrics()}
              aria-label="Hide the desktop lyrics"
              title="Hide (Ctrl+Shift+D)"
              className="grid h-7 w-7 place-items-center rounded-full text-white/70 hover:bg-white/15 hover:text-white"
            >
              <IconClose width={15} height={15} />
            </button>
          </motion.div>
        )}
      </AnimatePresence>
      <motion.div animate={{ opacity: shown || editing ? 1 : 0 }} transition={{ duration: 0.6 }} className="w-full select-none">
        {editing && !shown ? (
          <LyricText text={song ? 'Lyrics show here as the song plays' : 'Play a song with lyrics'} look={look.look} now progress={1} colour={colour} px={px * 0.7} />
        ) : (
          <>
            <AnimatePresence mode="popLayout" initial={false}>
              <motion.div key={`n${at}`} initial={{ opacity: 0, y: 14 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -14 }} transition={{ duration: 0.3 }}>
                <LyricText text={now ?? ''} latin={latinOf(now, look.latin)} look={look.look} now progress={progress} colour={colour} px={px} />
              </motion.div>
            </AnimatePresence>
            {look.lines === 2 && next !== undefined && (
              <AnimatePresence mode="popLayout" initial={false}>
                <motion.div key={`x${at}`} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: 0.3 }} className="mt-1">
                  <LyricText text={next} look={look.look === 'karaoke' ? 'clean' : look.look} now={false} colour={colour} px={px * 0.7} />
                </motion.div>
              </AnimatePresence>
            )}
          </>
        )}
      </motion.div>
    </div>
  )
}
