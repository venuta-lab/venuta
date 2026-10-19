import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { inApp } from '../../lib/engine'
import { useLyricLook, type Ground, type LyricWindow } from '../../lib/lyricLook'
import {
  BACKGROUND_NAMES,
  BACKGROUNDS,
  closeLyricWindow,
  groundOf,
  keepWindowSettings,
  setLyricWindow,
  themeOf,
  useLyricWindow,
  useThemes,
  type LyricBackground,
  type LyricTheme,
} from '../../lib/lyricWindows'
import { seeThrough, type Px } from '../../lib/lyricWindowSettings'
import { setWindowTheme, usePrefs } from '../../lib/prefs'
import { Backdrop } from '../Backdrop'
import { IconBackdrop, IconLock, IconLyricStyle, IconPin, IconUnlock } from '../Icons'
import { ThemeFx } from '../ThemeFx'
import { LookPanel, LookStrip } from './LookPanel'
import { hasIndic, useLyricColour, useNowLyrics, useWindowCover } from './useNowLyrics'

/*
  A lyrics window's frame. Each window has one job:
    the big screen  fills a screen, for a TV, a second monitor, a room: its controls come with
                    the mouse and go when it rests; Esc closes it.
    desktop lyrics  a strip over everything while you do something else: moved from anywhere,
                    sized from every edge, on top or not, and locked so clicks go through it (all
                    but its lock button, which shows as the pointer comes over it: the engine
                    watches the pointer, lyric_windows.rs).
  Both have the same three choices: what's behind the words (Clear, Glass, Frosted, the cover's
  light, the theme's scene), the theme (Venuta's own unless another is chosen), and the look.
*/

type ResizeDirection = Parameters<ReturnType<typeof getCurrentWindow>['startResizeDragging']>[0]

/** How long the big screen's controls stay after the mouse stops. */
const IDLE_MS = 2500
/** The desktop lyrics' top band while locked: room for the lock. Unlocked, it fits their bar
 *  (one row, or two on a narrow window), kept even while the bar is hidden, so the words never
 *  move as the pointer comes and goes. */
const BAND = 46

/** What the frame tells the window inside it. */
export interface Frame {
  /** Only the words (Clear): nothing else is drawn. */
  clear: boolean
  ground: Ground
  colour: string
  locked: boolean
  /** The controls are showing (the mouse moved lately, or unlocked desktop lyrics). */
  awake: boolean
}

type Panel = 'style' | 'background' | 'theme'

export function LyricsWindow(props: { win: LyricWindow; children: (f: Frame) => ReactNode }) {
  const { win } = props
  const stage = win === 'stage'
  const s = useLyricWindow(win)
  // A theme's light or dark mode follows Venuta's choice, so the prefs are listened to.
  const prefs = usePrefs()
  const look = useLyricLook(win)
  const { song, root, lyrics } = useNowLyrics()
  const ground = groundOf(s)
  const colour = useLyricColour(look.colour, root, song?.dir, ground)
  const cover = useWindowCover()
  const bg = s.background

  // The theme on screen here: the one chosen, or Venuta's own.
  useEffect(() => setWindowTheme(themeOf(s.theme)), [s.theme])
  // Frosted: Windows blurs what's behind the window (the page then tints it). A theme whose
  // ground lets what's behind show (Lumen's glass) gets the same blur, as Venuta's window has.
  useEffect(() => {
    if (!inApp()) return
    const ground = getComputedStyle(document.documentElement).getPropertyValue('--app-bg')
    const on = bg === 'frosted' || (bg === 'theme' && seeThrough(ground))
    void invoke('lyrics_window_frost', { kind: win, on }).catch(() => {})
  }, [win, bg, s.theme, prefs])

  // The desktop lyrics' place, kept for next time.
  useEffect(() => {
    if (!inApp() || stage) return
    const w = getCurrentWindow()
    let t = 0
    const keep = () => {
      window.clearTimeout(t)
      t = window.setTimeout(async () => {
        // Screen pixels as they are: the same spot on every screen, whatever its scale.
        const [pos, size] = await Promise.all([w.outerPosition(), w.outerSize()])
        keepWindowSettings(win, { px: [pos.x, pos.y, size.width, size.height].map(Math.round) as Px })
      }, 250)
    }
    const off = [w.onMoved(keep), w.onResized(keep)]
    return () => {
      window.clearTimeout(t)
      off.forEach((p) => void p.then((un) => un()))
    }
  }, [win, stage])

  // The controls: always on the unlocked desktop lyrics (they're being arranged); on the big
  // screen, while the mouse moves or a panel is open.
  const [moved, setMoved] = useState(true)
  const [panel, setPanel] = useState<Panel | null>(null)
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
  const unlocked = !s.locked
  // The unlocked desktop lyrics show their controls while the pointer is over them (or a list of
  // theirs is open): away from them, they look exactly as they will.
  const [hover, setHover] = useState(false)
  const [focus, setFocus] = useState(false)
  const awake = unlocked && (stage ? moved || panel !== null : hover || focus)
  // The bar's height, for the band.
  const bar = useRef<HTMLDivElement>(null)
  const [barH, setBarH] = useState(0)
  useLayoutEffect(() => {
    const b = bar.current
    if (!b) return
    const o = new ResizeObserver(() => setBarH(b.offsetHeight))
    o.observe(b)
    return () => o.disconnect()
  }, [unlocked, stage])
  const band = unlocked && barH ? Math.max(BAND, barH + 10) : BAND

  // Esc closes a panel, then the big screen.
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      if (panel) setPanel(null)
      else if (stage) void closeLyricWindow(win)
    }
    window.addEventListener('keydown', key)
    return () => window.removeEventListener('keydown', key)
  }, [panel, stage, win])

  // The desktop lyrics' corners are the ones Windows rounds (lyric_windows.rs: 8 px), so the
  // blur, the theme's ground and the page share one edge; the big screen fills a screen, square.
  const radius = stage ? 0 : 8
  return (
    <div
      className="relative h-full w-full overflow-hidden"
      style={{
        cursor: stage && !awake ? 'none' : unlocked && !stage ? 'move' : 'default',
        color: ground === 'dark' ? '#fff' : 'var(--ink)',
        borderRadius: radius,
      }}
      onMouseEnter={() => setHover(true)}
      onMouseMove={() => !hover && setHover(true)}
      onMouseLeave={() => setHover(false)}
      onFocus={() => setFocus(true)}
      onBlur={(e) => !e.currentTarget.contains(e.relatedTarget as Node | null) && setFocus(false)}
      onMouseDown={(e) => {
        // The unlocked desktop lyrics: dragged from anywhere but their controls.
        if (!stage && unlocked && e.button === 0 && !(e.target as HTMLElement).closest('button,select,input,[role=slider],[role=radiogroup],[data-edge]'))
          void getCurrentWindow().startDragging()
      }}
    >
      <Behind background={bg} ground={ground} cover={cover?.src} radius={radius} />
      {/* Being arranged: a fine edge and its four corners, so it can be found and sized. */}
      <AnimatePresence>{!stage && awake && <Arranging key="edge" ground={ground} radius={radius} clear={bg === 'clear'} />}</AnimatePresence>

      {stage ? (
        <div className="relative h-full w-full">{props.children({ clear: bg === 'clear', ground, colour, locked: s.locked, awake })}</div>
      ) : (
        <>
          {/* The top band: the bar while arranging (the lock while locked). Laid out whenever
              unlocked, shown while the pointer is over them. */}
          {unlocked && (
            <motion.div
              ref={bar}
              initial={false}
              animate={{ opacity: awake ? 1 : 0, y: awake ? 0 : -4 }}
              transition={{ duration: 0.16 }}
              className="absolute inset-x-0 top-0 z-20 flex justify-center px-2 pt-1.5"
              style={{ pointerEvents: awake ? 'auto' : 'none' }}
            >
              <StripBar win={win} onTop={s.onTop} background={bg} theme={s.theme}>
                <LookStrip win={win} ground={ground} coverColour={colour} indic={hasIndic(lyrics?.lines)} />
              </StripBar>
            </motion.div>
          )}
          <div className="absolute inset-x-0 bottom-0" style={{ top: band }}>
            {props.children({ clear: bg === 'clear', ground, colour, locked: s.locked, awake })}
          </div>
        </>
      )}

      {!stage && unlocked && <Edges />}
      {!stage && s.locked && <LockButton win={win} />}

      {/* The big screen's controls, in the corner. */}
      <AnimatePresence>
        {stage && awake && (
          <motion.div
            initial={{ opacity: 0, y: -6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }}
            className="absolute right-[3vw] top-[3vh] z-20 flex flex-col items-end gap-3"
          >
            <StageBar win={win} panel={panel} setPanel={setPanel} />
            <AnimatePresence>
              {panel && (
                <motion.div
                  initial={{ opacity: 0, y: -6, scale: 0.97 }}
                  animate={{ opacity: 1, y: 0, scale: 1 }}
                  exit={{ opacity: 0, y: -6, scale: 0.97 }}
                  transition={{ type: 'spring', stiffness: 420, damping: 32 }}
                  style={{ transformOrigin: 'top right' }}
                  className="max-h-[80vh] w-[360px] overflow-y-auto rounded-[24px] bg-[rgb(14_14_18/0.86)] p-4 text-white shadow-[0_24px_60px_-20px_rgb(0_0_0/0.9),inset_0_0_0_1px_rgb(255_255_255/0.1)]"
                >
                  {panel === 'style' && <LookPanel win={win} ground={ground} tone="dark" coverColour={colour} indic={hasIndic(lyrics?.lines)} />}
                  {panel === 'background' && <BackgroundTiles win={win} value={bg} />}
                  {panel === 'theme' && <ThemeTiles win={win} value={s.theme} />}
                </motion.div>
              )}
            </AnimatePresence>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  )
}

/**
 * What's behind the words. Clear: nothing. Glass: see-through, a light tint with a glossy edge,
 * what's behind still sharp. Frosted: Windows blurs what's behind, and this tints it (milky on a
 * light theme, smoky on a dark one). Cover: the cover's light. Theme: the theme's own ground and
 * scene, as behind the main window.
 */
function Behind(props: { background: LyricBackground; ground: Ground; cover: string | undefined; radius: number }) {
  const light = props.ground === 'light'
  switch (props.background) {
    case 'clear':
      return null
    case 'glass':
      return (
        <div
          aria-hidden
          className="absolute inset-0"
          style={{
            borderRadius: props.radius,
            // A pane of glass: what's behind stays sharp under a clear tint, a sheen across its
            // top and a bright edge; the theme's accent just tints it.
            background: light
              ? 'linear-gradient(172deg, rgb(255 255 255 / 0.55), rgb(255 255 255 / 0.22) 38%, rgb(255 255 255 / 0.3)), color-mix(in oklab, var(--accent) 6%, rgb(255 255 255 / 0.18))'
              : 'linear-gradient(172deg, rgb(255 255 255 / 0.2), rgb(255 255 255 / 0.05) 38%, rgb(255 255 255 / 0.08)), color-mix(in oklab, var(--accent) 8%, rgb(14 16 22 / 0.34))',
            boxShadow: light
              ? 'inset 0 1px 0 rgb(255 255 255 / 0.95), inset 0 0 0 1px rgb(255 255 255 / 0.65), inset 0 -24px 40px -30px rgb(0 0 0 / 0.14)'
              : 'inset 0 1px 0 rgb(255 255 255 / 0.45), inset 0 0 0 1px rgb(255 255 255 / 0.22), inset 0 -24px 40px -30px rgb(0 0 0 / 0.5)',
          }}
        />
      )
    case 'frosted':
      return (
        <div
          aria-hidden
          className="absolute inset-0"
          style={{
            background: light
              ? 'linear-gradient(180deg, rgb(250 250 252 / 0.62), rgb(244 245 248 / 0.5))'
              : 'linear-gradient(180deg, rgb(22 24 30 / 0.46), rgb(14 16 22 / 0.56))',
            boxShadow: light ? 'inset 0 0 0 1px rgb(255 255 255 / 0.5)' : 'inset 0 0 0 1px rgb(255 255 255 / 0.08)',
          }}
        />
      )
    case 'cover':
      return <CoverLight src={props.cover} />
    case 'theme':
      return (
        <>
          <div aria-hidden className="absolute inset-0" style={{ backgroundColor: 'var(--app-bg)', backgroundImage: 'var(--app-image)', backgroundSize: 'cover' }} />
          <Backdrop covers={[]} focus={props.cover ?? null} />
          <ThemeFx />
        </>
      )
  }
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

/** The unlocked desktop lyrics while the pointer is over them: a fine edge, its four corners
 *  marked (they size it), and on Clear a faint fill so the strip can be seen at all. */
function Arranging({ ground, radius, clear }: { ground: Ground; radius: number; clear: boolean }) {
  const line = ground === 'dark' ? 'rgb(255 255 255 / 0.35)' : 'rgb(0 0 0 / 0.2)'
  const mark = ground === 'dark' ? 'rgb(255 255 255 / 0.9)' : 'rgb(0 0 0 / 0.55)'
  const corner = (place: string, sides: string) => (
    <span className={`absolute h-3.5 w-3.5 ${place}`} style={{ borderColor: mark, borderStyle: 'solid', borderWidth: sides, borderRadius: 3 }} />
  )
  return (
    <motion.div
      aria-hidden
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      transition={{ duration: 0.16 }}
      className="pointer-events-none absolute inset-0 z-10"
      style={{ borderRadius: radius, boxShadow: `inset 0 0 0 1px ${line}`, background: clear ? 'rgb(0 0 0 / 0.18)' : undefined }}
    >
      {corner('left-1 top-1', '2px 0 0 2px')}
      {corner('right-1 top-1', '2px 2px 0 0')}
      {corner('bottom-1 left-1', '0 0 2px 2px')}
      {corner('bottom-1 right-1', '0 2px 2px 0')}
    </motion.div>
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
      aria-label="Unlock: move, size and change them"
      title="Unlock: move, size and change them"
      className="absolute right-3 top-3 z-40 grid h-9 w-9 place-items-center rounded-full bg-[rgb(14_14_18/0.8)] text-white shadow-[0_6px_20px_-6px_rgb(0_0_0/0.8),inset_0_0_0_1px_rgb(255_255_255/0.15)] transition-[opacity,transform] duration-200"
      style={{ opacity: pointer.inside ? 1 : 0, transform: pointer.over ? 'scale(1.08)' : 'scale(1)' }}
    >
      <IconLock width={16} height={16} strokeWidth={2.1} />
    </button>
  )
}

// No CSS backdrop blur in a see-through window: it samples the window's empty pixels and smears
// them white. Near-opaque instead.
const BAR = 'flex max-w-full flex-wrap items-center justify-center gap-0.5 rounded-[20px] bg-[rgb(14_14_18/0.82)] p-1 text-white shadow-[0_10px_30px_-10px_rgb(0_0_0/0.8),inset_0_0_0_1px_rgb(255_255_255/0.1)]'
const ON = 'bg-white/20 text-white'
/** A button in words (never icons that read as a window's own buttons). */
const WORD = 'flex h-8 items-center gap-1.5 whitespace-nowrap rounded-full px-2.5 text-[12.5px] font-medium text-white/80 transition-colors hover:bg-white/15 hover:text-white'
const RULE = <span className="mx-1 h-4 w-px bg-white/15" aria-hidden />

/** The big screen's controls: its style, background and theme, each a panel; and Close. */
function StageBar(props: { win: LyricWindow; panel: Panel | null; setPanel: (p: Panel | null) => void }) {
  const toggle = (p: Panel) => props.setPanel(props.panel === p ? null : p)
  const tab = (p: Panel, label: string, icon: ReactNode) => (
    <button onClick={() => toggle(p)} aria-expanded={props.panel === p} className={`${WORD} px-3 text-[13px] ${props.panel === p ? ON : ''}`}>
      {icon}
      {label}
    </button>
  )
  return (
    <div className={BAR}>
      {tab('style', 'Style', <IconLyricStyle width={17} height={17} />)}
      {tab('background', 'Background', <IconBackdrop width={16} height={16} />)}
      {tab('theme', 'Theme', <ThemeDot />)}
      {RULE}
      <button onClick={() => void closeLyricWindow(props.win)} title="Esc" className={`${WORD} px-3 text-[13px]`}>
        Close
      </button>
    </div>
  )
}

/** The theme's accent, as a dot (the Theme button's icon). */
const ThemeDot = () => <span aria-hidden className="h-3.5 w-3.5 rounded-full shadow-[inset_0_0_0_1.5px_rgb(255_255_255/0.5)]" style={{ background: 'var(--accent)' }} />

/** The desktop lyrics' controls, in their one bar: the lock, the background, the theme, the
 *  look (given), on top, and Hide. On a narrow window it wraps rather than leave anything out. */
function StripBar(props: { win: LyricWindow; onTop: boolean; background: LyricBackground; theme: LyricTheme; children: ReactNode }) {
  const { win } = props
  const themes = useThemes()
  const select = 'h-7 max-w-[12em] cursor-pointer rounded-full bg-white/10 px-2 text-[12.5px] text-white outline-none hover:bg-white/15'
  return (
    <div className={BAR}>
      <button
        onClick={() => void setLyricWindow(win, { locked: true })}
        className="flex h-8 items-center gap-1.5 rounded-full bg-white pl-2.5 pr-3 text-[12.5px] font-semibold text-black transition-transform active:scale-95"
        title="Clicks go through them. Point at them to find their lock again."
      >
        <IconUnlock width={14} height={14} strokeWidth={2.2} />
        Lock
      </button>
      {RULE}
      <label className="flex items-center gap-1.5 pl-1.5 pr-0.5" title="What's behind the words">
        <IconBackdrop width={16} height={16} className="text-white/70" />
        <select value={props.background} onChange={(e) => void setLyricWindow(win, { background: e.target.value as LyricBackground })} aria-label="Background" className={select}>
          {BACKGROUNDS.map((b) => (
            <option key={b} value={b} className="bg-neutral-900 text-white">
              {BACKGROUND_NAMES[b].name}
            </option>
          ))}
        </select>
      </label>
      <label className="flex items-center gap-1.5 pl-1.5 pr-0.5" title="The theme: the colours, the type, the tint of glass and frost">
        <ThemeDot />
        <select value={props.theme} onChange={(e) => void setLyricWindow(win, { theme: e.target.value as LyricTheme })} aria-label="Theme" className={select}>
          {themes.map((t) => (
            <option key={t.id} value={t.id} className="bg-neutral-900 text-white">
              {t.name}
            </option>
          ))}
        </select>
      </label>
      {RULE}
      {props.children}
      {RULE}
      <button
        onClick={() => void setLyricWindow(win, { onTop: !props.onTop })}
        aria-pressed={props.onTop}
        title="Kept over every window"
        className={`${WORD} ${props.onTop ? ON : ''}`}
      >
        <IconPin />
        On top
      </button>
      <button onClick={() => void closeLyricWindow(win)} title="Ctrl+Shift+D" className={WORD}>
        Hide
      </button>
    </div>
  )
}

const TILE = (chosen: boolean) => `rounded-[12px] px-3 py-2 text-left text-[12.5px] transition-colors ${chosen ? 'bg-white text-black' : 'bg-white/[0.08] text-white/85 hover:bg-white/[0.14]'}`

/** The big screen's background panel: the five, each a tile. */
function BackgroundTiles({ win, value }: { win: LyricWindow; value: LyricBackground }) {
  return (
    <div className="grid gap-3">
      <div className="text-[13px] font-semibold">Background</div>
      <div role="radiogroup" aria-label="Background" className="grid grid-cols-2 gap-2">
        {BACKGROUNDS.map((b) => (
          <button key={b} role="radio" aria-checked={value === b} onClick={() => void setLyricWindow(win, { background: b })} className={TILE(value === b)}>
            <span className="block truncate font-medium">{BACKGROUND_NAMES[b].name}</span>
            <span className={`block truncate text-[11px] ${value === b ? 'text-black/60' : 'text-white/50'}`}>{BACKGROUND_NAMES[b].line}</span>
          </button>
        ))}
      </div>
    </div>
  )
}

/** The big screen's theme panel: Venuta's own first, then every theme. */
function ThemeTiles({ win, value }: { win: LyricWindow; value: LyricTheme }) {
  const themes = useThemes()
  return (
    <div className="grid gap-3">
      <div className="text-[13px] font-semibold">Theme</div>
      <div role="radiogroup" aria-label="Theme" className="grid grid-cols-2 gap-2">
        {themes.map((t) => (
          <button key={t.id} role="radio" aria-checked={value === t.id} title={t.line} onClick={() => void setLyricWindow(win, { theme: t.id })} className={`${TILE(value === t.id)} ${t.id === 'venuta' ? 'col-span-2' : ''}`}>
            <span className="block truncate font-medium">{t.name}</span>
            <span className={`block truncate text-[11px] ${value === t.id ? 'text-black/60' : 'text-white/50'}`}>{t.line}</span>
          </button>
        ))}
      </div>
    </div>
  )
}
