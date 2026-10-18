import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { inApp } from '../../lib/engine'
import type { Ground, LyricWindow } from '../../lib/lyricLook'
import { closeLyricWindow, groundOf, keepWindowSettings, setLyricWindow, themeOf, useBackgrounds, useLyricWindow, type LyricBackground } from '../../lib/lyricWindows'
import { setWindowTheme, usePrefs } from '../../lib/prefs'
import { Backdrop } from '../Backdrop'
import { IconBackdrop, IconClose, IconFullScreen, IconLeaveFullScreen, IconLock, IconLyricStyle, IconOnTop, IconUnlock } from '../Icons'
import { ThemeFx } from '../ThemeFx'
import { LookPanel, LookStrip } from './LookPanel'
import { hasIndic, useLyricColour, useNowLyrics, useWindowCover } from './useNowLyrics'
import { useLyricLook } from '../../lib/lyricLook'

/*
  A lyrics window's frame, the same for the big screen and the desktop lyrics: what's behind the
  words (nothing, the cover's light, or a theme), moving it, sizing it from any edge, full screen
  or a window, on top or not, and its lock. Locked, clicks go through it: only its lock button,
  which shows as the pointer comes over the window, takes them (the engine watches the pointer,
  lyric_windows.rs). Unlocked, its toolbar has the rest.
*/

type ResizeDirection = Parameters<ReturnType<typeof getCurrentWindow>['startResizeDragging']>[0]

/** How long the big screen's controls stay after the mouse stops. */
const IDLE_MS = 2500

/** What the frame tells the window inside it. */
export interface Frame {
  /** Only the words (Clear): nothing else is drawn. */
  clear: boolean
  ground: Ground
  colour: string
  locked: boolean
  full: boolean
  /** The controls are showing (the mouse moved lately, or unlocked desktop lyrics). */
  awake: boolean
}

export function LyricsWindow(props: { win: LyricWindow; children: (f: Frame) => ReactNode }) {
  const { win } = props
  const s = useLyricWindow(win)
  // A theme's light or dark mode follows Venuta's choice, so the prefs are listened to.
  usePrefs()
  const look = useLyricLook(win)
  const { song, root, lyrics } = useNowLyrics()
  const ground = groundOf(s.background)
  const colour = useLyricColour(look.colour, root, song?.dir, ground)
  const cover = useWindowCover()
  const clear = s.background === 'clear'
  const themed = s.background !== 'clear' && s.background !== 'cover'

  // The theme on screen here: the one chosen, or Venuta's own.
  useEffect(() => setWindowTheme(themeOf(s.background)), [s.background])

  // Full screen, as the window really is (F11, a double-click, the button, Esc).
  const full = s.full
  const toggleFull = () => void setLyricWindow(win, { full: !full })

  // Where it is as a window, kept for next time (never while it fills a screen).
  useEffect(() => {
    if (!inApp()) return
    const w = getCurrentWindow()
    let t = 0
    const keep = () => {
      window.clearTimeout(t)
      t = window.setTimeout(async () => {
        if (await w.isFullscreen()) return
        const [pos, size, scale] = await Promise.all([w.outerPosition(), w.outerSize(), w.scaleFactor()])
        keepWindowSettings(win, { bounds: [pos.x / scale, pos.y / scale, size.width / scale, size.height / scale] })
      }, 250)
    }
    const off = [w.onMoved(keep), w.onResized(keep)]
    return () => {
      window.clearTimeout(t)
      off.forEach((p) => void p.then((un) => un()))
    }
  }, [win])

  // The controls: always on the unlocked desktop lyrics (they're being arranged); on the big
  // screen, while the mouse moves or a panel is open.
  const [moved, setMoved] = useState(true)
  const [panel, setPanel] = useState<'style' | 'background' | null>(null)
  const idle = useRef(0)
  useEffect(() => {
    const wake = () => {
      setMoved(true)
      window.clearTimeout(idle.current)
      idle.current = window.setTimeout(() => setMoved(false), IDLE_MS)
    }
    wake()
    window.addEventListener('mousemove', wake)
    return () => {
      window.removeEventListener('mousemove', wake)
      window.clearTimeout(idle.current)
    }
  }, [])
  const awake = !s.locked && (win === 'overlay' || moved || panel !== null)

  // Keys: Esc closes a panel, then leaves full screen, then closes the big screen; F11 fills or
  // leaves the screen.
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        if (panel) setPanel(null)
        else if (full) toggleFull()
        else if (win === 'stage') void closeLyricWindow(win)
      } else if (e.key === 'F11') {
        e.preventDefault()
        toggleFull()
      }
    }
    window.addEventListener('keydown', key)
    return () => window.removeEventListener('keydown', key)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [panel, full, win])

  const unlocked = !s.locked
  return (
    <div
      className="relative h-full w-full overflow-hidden"
      style={{
        cursor: win === 'stage' && !awake && !s.locked ? 'none' : unlocked && win === 'overlay' ? 'move' : 'default',
        color: ground === 'dark' ? '#fff' : 'var(--ink)',
        borderRadius: full ? 0 : 14,
      }}
      onMouseDown={(e) => {
        // Unlocked: dragged from anywhere but its controls.
        if (unlocked && !full && e.button === 0 && !(e.target as HTMLElement).closest('button,select,input,[role=slider],[role=radiogroup],[data-edge]'))
          void getCurrentWindow().startDragging()
      }}
      onDoubleClick={(e) => {
        if (unlocked && !(e.target as HTMLElement).closest('button,select,input,[role=slider]')) toggleFull()
      }}
    >
      {/* What's behind the words. */}
      {s.background === 'cover' && <CoverLight src={cover?.src} />}
      {themed && (
        <>
          {/* The theme's own ground, as behind the main window. */}
          <div aria-hidden className="absolute inset-0" style={{ backgroundColor: 'var(--app-bg)', backgroundImage: 'var(--app-image)', backgroundSize: 'cover' }} />
          <Backdrop covers={[]} focus={cover?.src ?? null} />
          <ThemeFx />
        </>
      )}
      {/* Unlocked and see-through, its edge shows, so it can be found, moved and sized. */}
      {unlocked && !full && (
        <div
          aria-hidden
          className="pointer-events-none absolute inset-0 z-10 rounded-[14px]"
          style={{ outline: `2px dashed ${ground === 'dark' ? 'rgb(255 255 255 / 0.55)' : 'rgb(0 0 0 / 0.3)'}`, outlineOffset: -4, background: clear ? 'rgb(0 0 0 / 0.28)' : undefined }}
        />
      )}

      <div className={win === 'overlay' ? 'relative flex h-full w-full flex-col' : 'relative h-full w-full'}>
        {/* The desktop lyrics' controls sit above the words (the words make room). */}
        <AnimatePresence initial={false}>
          {win === 'overlay' && awake && (
            <motion.div key="bar" initial={{ opacity: 0, height: 0 }} animate={{ opacity: 1, height: 'auto' }} exit={{ opacity: 0, height: 0 }} className="relative z-20 flex shrink-0 justify-center px-2 pt-2">
              <Toolbar win={win} full={full} onTop={s.onTop} panel={panel} setPanel={setPanel} background={s.background} toggleFull={toggleFull}>
                <span className="mx-1 h-4 w-px bg-white/15" aria-hidden />
                <LookStrip win={win} ground={ground} coverColour={colour} indic={hasIndic(lyrics?.lines)} />
              </Toolbar>
            </motion.div>
          )}
        </AnimatePresence>
        <div className={win === 'overlay' ? 'relative min-h-0 flex-1' : 'relative h-full w-full'}>{props.children({ clear, ground, colour, locked: s.locked, full, awake })}</div>
      </div>

      {unlocked && !full && <Edges />}

      {/* Locked: the one thing that takes a click. */}
      {s.locked && <LockButton win={win} />}

      {/* Unlocked: the toolbar. */}
      <AnimatePresence>
        {win === 'stage' && awake && (
          <motion.div
            initial={{ opacity: 0, y: -6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }}
            className="absolute right-[3vw] top-[3vh] z-20 flex flex-col items-end gap-3"
          >
            <Toolbar win={win} full={full} onTop={s.onTop} panel={panel} setPanel={setPanel} background={s.background} toggleFull={toggleFull} />
            <AnimatePresence>
              {panel && (
                <motion.div
                  initial={{ opacity: 0, y: -6, scale: 0.97 }}
                  animate={{ opacity: 1, y: 0, scale: 1 }}
                  exit={{ opacity: 0, y: -6, scale: 0.97 }}
                  transition={{ type: 'spring', stiffness: 420, damping: 32 }}
                  style={{ transformOrigin: 'top right' }}
                  className="max-h-[80vh] w-[360px] overflow-y-auto rounded-[24px] bg-black/60 p-4 text-white shadow-[0_24px_60px_-20px_rgb(0_0_0/0.9),inset_0_0_0_1px_rgb(255_255_255/0.1)] backdrop-blur-2xl"
                >
                  {panel === 'style' ? (
                    <LookPanel win={win} ground={ground} tone="dark" coverColour={colour} indic={hasIndic(lyrics?.lines)} />
                  ) : (
                    <BackgroundGrid win={win} value={s.background} />
                  )}
                </motion.div>
              )}
            </AnimatePresence>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  )
}

/** The cover as light behind everything (the big screen's own look). */
function CoverLight({ src }: { src: string | undefined }) {
  return (
    <>
      <div aria-hidden className="absolute inset-0 bg-black" />
      <AnimatePresence>
        {src && (
          <motion.img
            key={src}
            src={src}
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
      <div aria-hidden className="absolute inset-0" style={{ background: 'radial-gradient(120% 90% at 50% 45%, transparent 30%, rgb(0 0 0 / 0.65))' }} />
    </>
  )
}

/** Thin zones along every edge and corner: drag one to size the window. */
function Edges() {
  const zones: [ResizeDirection, string][] = [
    ['North', 'left-3 right-3 top-0 h-[6px] cursor-n-resize'],
    ['South', 'bottom-0 left-3 right-3 h-[6px] cursor-s-resize'],
    ['West', 'bottom-3 left-0 top-3 w-[6px] cursor-w-resize'],
    ['East', 'bottom-3 right-0 top-3 w-[6px] cursor-e-resize'],
    ['NorthWest', 'left-0 top-0 h-3 w-3 cursor-nw-resize'],
    ['NorthEast', 'right-0 top-0 h-3 w-3 cursor-ne-resize'],
    ['SouthWest', 'bottom-0 left-0 h-3 w-3 cursor-sw-resize'],
    ['SouthEast', 'bottom-0 right-0 h-3 w-3 cursor-se-resize'],
  ]
  return (
    <>
      {zones.map(([dir, place]) => (
        <div
          key={dir}
          data-edge
          aria-hidden
          className={`absolute z-30 ${place}`}
          onMouseDown={(e) => {
            if (e.button !== 0) return
            e.stopPropagation()
            void getCurrentWindow().startResizeDragging(dir)
          }}
        />
      ))}
    </>
  )
}

/**
 * Locked: a small lock in the corner, showing while the pointer is over the window (the engine
 * says when, since a locked window hears no mouse). Over it, the window takes the click: open it.
 */
function LockButton({ win }: { win: LyricWindow }) {
  const ref = useRef<HTMLButtonElement>(null)
  const [pointer, setPointer] = useState({ inside: false, over: false })
  // Where the button is, for the engine to know when the pointer is on it.
  useLayoutEffect(() => {
    if (!inApp()) return
    const tell = () => {
      const r = ref.current?.getBoundingClientRect()
      if (r) void invoke('lyrics_lock_spot', { kind: win, spot: [r.left, r.top, r.width, r.height] }).catch(() => {})
    }
    tell()
    window.addEventListener('resize', tell)
    return () => {
      window.removeEventListener('resize', tell)
      void invoke('lyrics_lock_spot', { kind: win, spot: null }).catch(() => {})
    }
  }, [win])
  useEffect(() => {
    if (!inApp()) return
    const off = listen<{ inside: boolean; over: boolean }>('lyrics-pointer', (e) => setPointer(e.payload))
    return () => void off.then((u) => u())
  }, [])
  return (
    <button
      ref={ref}
      onClick={() => void setLyricWindow(win, { locked: false })}
      aria-label="Unlock: move, size and change it"
      title="Unlock: move, size and change it"
      className="absolute right-3 top-3 z-40 grid h-9 w-9 place-items-center rounded-full bg-black/60 text-white shadow-[0_6px_20px_-6px_rgb(0_0_0/0.8),inset_0_0_0_1px_rgb(255_255_255/0.15)] backdrop-blur-xl transition-[opacity,transform] duration-200"
      style={{ opacity: pointer.inside ? 1 : 0, transform: pointer.over ? 'scale(1.08)' : 'scale(1)' }}
    >
      <IconLock width={16} height={16} strokeWidth={2.1} />
    </button>
  )
}

const BTN = 'grid h-8 w-8 place-items-center rounded-full text-white/80 transition-colors hover:bg-white/15 hover:text-white'
const ON = 'bg-white/20 text-white'

function Toolbar(props: {
  win: LyricWindow
  full: boolean
  onTop: boolean
  background: LyricBackground
  panel: 'style' | 'background' | null
  setPanel: (p: 'style' | 'background' | null) => void
  toggleFull: () => void
  /** More, after the background (the desktop lyrics' look, in their one bar). */
  children?: ReactNode
}) {
  const { win } = props
  const toggle = (p: 'style' | 'background') => props.setPanel(props.panel === p ? null : p)
  return (
    // One bar; on a narrow window it wraps into a second row rather than leave anything out.
    <div className="flex max-w-full flex-wrap items-center justify-center gap-0.5 rounded-[20px] bg-black/65 p-1 text-white shadow-[0_10px_30px_-10px_rgb(0_0_0/0.8),inset_0_0_0_1px_rgb(255_255_255/0.1)] backdrop-blur-xl">
      <button
        onClick={() => void setLyricWindow(win, { locked: true })}
        className="flex h-8 items-center gap-1.5 rounded-full bg-white pl-2.5 pr-3 text-[12.5px] font-semibold text-black transition-transform active:scale-95"
        title="Clicks go through it. Point at it to find its lock again."
      >
        <IconUnlock width={14} height={14} strokeWidth={2.2} />
        Lock
      </button>
      <span className="mx-1 h-4 w-px bg-white/15" aria-hidden />
      {win === 'stage' && (
        <button onClick={() => toggle('style')} aria-expanded={props.panel === 'style'} className={`${BTN} w-auto gap-1.5 px-3 text-[12.5px] font-medium ${props.panel === 'style' ? ON : ''}`}>
          <IconLyricStyle width={17} height={17} />
          Style
        </button>
      )}
      {win === 'stage' ? (
        <button onClick={() => toggle('background')} aria-expanded={props.panel === 'background'} className={`${BTN} w-auto gap-1.5 px-3 text-[12.5px] font-medium ${props.panel === 'background' ? ON : ''}`}>
          <IconBackdrop width={16} height={16} />
          Background
        </button>
      ) : (
        <BackgroundSelect win={win} value={props.background} />
      )}
      {props.children}
      <span className="mx-1 h-4 w-px bg-white/15" aria-hidden />
      <button onClick={props.toggleFull} aria-label={props.full ? 'Back to a window' : 'Fill the screen'} title={props.full ? 'Back to a window (Esc)' : 'Fill the screen (F11)'} className={BTN}>
        {props.full ? <IconLeaveFullScreen /> : <IconFullScreen />}
      </button>
      <button
        onClick={() => void setLyricWindow(win, { onTop: !props.onTop })}
        aria-pressed={props.onTop}
        aria-label="Over every window"
        title={props.onTop ? 'Over every window: on' : 'Over every window: off'}
        className={`${BTN} ${props.onTop ? ON : ''}`}
      >
        <IconOnTop />
      </button>
      <button onClick={() => void closeLyricWindow(win)} aria-label="Close" title={win === 'stage' ? 'Close (Esc)' : 'Hide (Ctrl+Shift+D)'} className={BTN}>
        <IconClose width={15} height={15} />
      </button>
    </div>
  )
}

/** The big screen's background panel: every background, each a tile. */
function BackgroundGrid({ win, value }: { win: LyricWindow; value: LyricBackground }) {
  const all = useBackgrounds()
  return (
    <div className="grid gap-3">
      <div className="text-[13px] font-semibold">Background</div>
      <div role="radiogroup" aria-label="Background" className="grid grid-cols-2 gap-2">
        {all.map((b) => {
          const chosen = value === b.id
          return (
            <button
              key={b.id}
              role="radio"
              aria-checked={chosen}
              title={b.line}
              onClick={() => void setLyricWindow(win, { background: b.id })}
              className={`rounded-[12px] px-3 py-2 text-left text-[12.5px] transition-colors ${chosen ? 'bg-white text-black' : 'bg-white/[0.08] text-white/85 hover:bg-white/[0.14]'}`}
            >
              <span className="block truncate font-medium">{b.name}</span>
              <span className={`block truncate text-[11px] ${chosen ? 'text-black/60' : 'text-white/50'}`}>{b.line}</span>
            </button>
          )
        })}
      </div>
    </div>
  )
}

/** The desktop lyrics' background, in their one short row: a list (it opens past the window). */
function BackgroundSelect({ win, value }: { win: LyricWindow; value: LyricBackground }) {
  const all = useBackgrounds()
  return (
    <label className="flex items-center gap-1.5 pl-2 pr-1 text-[12.5px] text-white/80" title="What's behind the words">
      <IconBackdrop width={16} height={16} />
      <select
        value={value}
        onChange={(e) => void setLyricWindow(win, { background: e.target.value as LyricBackground })}
        aria-label="Background"
        className="h-7 max-w-[11em] cursor-pointer rounded-full bg-white/10 px-2 text-[12.5px] text-white outline-none hover:bg-white/15"
      >
        {all.map((b) => (
          <option key={b.id} value={b.id} className="bg-neutral-900 text-white">
            {b.name}
          </option>
        ))}
      </select>
    </label>
  )
}
