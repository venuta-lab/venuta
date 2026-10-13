import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window'
import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { loadCovers, setPlayingAccent, useCovers } from '../../lib/covers'
import { inApp } from '../../lib/engine'
import { clock, connectPlayer, current, miniPlayer, next, previous, seek, toggle, usePlayer } from '../../lib/player'
import { IconExpand, IconLyrics, IconNext, IconPause, IconPlay, IconPrevious } from '../Icons'
import { lineAt, useLyrics } from './NowPlaying'

/*
  The mini player: its own small window that stays on top. One window, three shapes, chosen by
  its size, so dragging an edge is enough to change it:
    micro  a strip (cover, title, play)
    mini   a bar (cover, title and artist or the line being sung, controls, a seek line)
    card   tall (the cover fills it; title and controls over it; a few lines of lyrics)
  Without a cover it shows a tile in the accent; without lyrics there is no lyrics button.
*/
type Shape = 'micro' | 'mini' | 'card'
const SIZES: Record<Shape, [number, number]> = { micro: [320, 64], mini: [380, 132], card: [300, 400] }
const shapeOf = (w: number, h: number): Shape => (h < 96 ? 'micro' : h >= 220 && w < h * 1.4 ? 'card' : 'mini')

const KEY = 'mini-player'
const saved = (): { lyrics: boolean } => {
  try {
    return { lyrics: false, ...JSON.parse(localStorage.getItem(KEY) ?? '{}') }
  } catch {
    return { lyrics: false }
  }
}

function useWindowSize() {
  const [size, setSize] = useState({ w: window.innerWidth, h: window.innerHeight })
  useEffect(() => {
    const on = () => setSize({ w: window.innerWidth, h: window.innerHeight })
    window.addEventListener('resize', on)
    return () => window.removeEventListener('resize', on)
  }, [])
  return size
}

export function MiniPlayer() {
  useEffect(() => connectPlayer(), [])
  const s = usePlayer()
  const song = current(s)
  const root = s.root ?? ''
  const { w, h } = useWindowSize()
  const shape = shapeOf(w, h)
  const [lyricsOn, setLyricsOn] = useState(saved().lyrics)
  useEffect(() => {
    try {
      localStorage.setItem(KEY, JSON.stringify({ lyrics: lyricsOn }))
    } catch {
      /* a convenience only */
    }
  }, [lyricsOn])

  // This window has its own cover cache: fetch the playing album's.
  const cover = useCovers(320)
  const dir = song?.dir ?? ''
  useEffect(() => {
    if (root && dir) void loadCovers(root, [dir])
  }, [root, dir])
  const art = dir ? cover(dir) : undefined
  useEffect(() => setPlayingAccent(art?.accent), [art?.accent])

  const showLyrics = lyricsOn && !!song?.lyrics && shape !== 'micro'
  const lyrics = useLyrics(root, showLyrics ? song?.path : undefined)
  const at = lineAt(lyrics, s.position_ms)
  const line = at >= 0 ? lyrics?.lines[at]?.[1] || '♪' : ''

  // Space plays and pauses; arrows skip.
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      if (e.key === ' ') {
        e.preventDefault()
        void toggle()
      } else if (e.key === 'ArrowRight' && e.ctrlKey) void next()
      else if (e.key === 'ArrowLeft' && e.ctrlKey) void previous()
    }
    window.addEventListener('keydown', on)
    return () => window.removeEventListener('keydown', on)
  }, [])

  // Drag the window from anywhere that isn't a control.
  const drag = (e: React.MouseEvent) => {
    if (e.button !== 0 || !inApp()) return
    if ((e.target as HTMLElement).closest('button, input, [data-no-drag]')) return
    void getCurrentWindow().startDragging()
  }
  const resize = (to: Shape) => inApp() && void getCurrentWindow().setSize(new LogicalSize(...SIZES[to]))
  const nextShape: Shape = shape === 'micro' ? 'mini' : shape === 'mini' ? 'card' : 'micro'

  const artist = song ? song.artists.join(', ') || song.album_artist : ''
  const duration = s.duration_ms || song?.duration_ms || 0
  const progress = duration ? Math.min(1, s.position_ms / duration) : 0

  return (
    <div onMouseDown={drag} className="group/mini relative h-full w-full select-none overflow-hidden text-ink">
      <Light src={art?.src} card={shape === 'card'} />

      {!song ? (
        <div className="relative grid h-full place-items-center px-4 text-center text-[13px] text-ink/60">
          Nothing playing. Choose music in Venuta.
          <WindowButtons shape={shape} nextShape={nextShape} onShape={resize} lyrics={null} />
        </div>
      ) : shape === 'micro' ? (
        <div className="relative flex h-full items-center gap-3 pl-2 pr-2">
          <Art src={art?.src} title={song.album} className="h-11 w-11 rounded-[10px]" />
          <div className="min-w-0 flex-1">
            <div className="truncate text-[13px] font-semibold leading-tight">{song.title}</div>
            <div className="truncate text-[11.5px] leading-tight text-ink/60">{artist}</div>
          </div>
          <PlayButton playing={s.playing} size={36} />
          <WindowButtons shape={shape} nextShape={nextShape} onShape={resize} lyrics={null} compact />
          <Progress value={progress} />
        </div>
      ) : shape === 'mini' ? (
        <div className="relative flex h-full items-center gap-3.5 p-3 pb-4">
          <Art src={art?.src} title={song.album} className="aspect-square h-full rounded-[14px]" />
          <div className="flex min-w-0 flex-1 flex-col justify-center gap-1.5 self-stretch">
            <div className="min-w-0 pr-16">
              <div className="truncate text-[14.5px] font-semibold leading-tight">{song.title}</div>
              <AnimatePresence mode="wait" initial={false}>
                <motion.div
                  key={showLyrics ? line : artist}
                  initial={{ opacity: 0, y: 4 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: -4 }}
                  transition={{ duration: 0.2 }}
                  className={`truncate text-[12.5px] leading-snug ${showLyrics ? 'text-accent-ink' : 'text-ink/60'}`}
                >
                  {showLyrics ? line || artist : artist}
                </motion.div>
              </AnimatePresence>
            </div>
            <Controls playing={s.playing} />
          </div>
          <WindowButtons
            shape={shape}
            nextShape={nextShape}
            onShape={resize}
            lyrics={song.lyrics ? { on: lyricsOn, toggle: () => setLyricsOn((o) => !o) } : null}
          />
          <Progress value={progress} duration={duration} />
        </div>
      ) : (
        <div className="relative flex h-full flex-col">
          {/* The cover fills the top; words and controls sit on a fade at the bottom. */}
          <div className="relative min-h-0 flex-1">
            {art ? (
              <img src={art.src} alt="" draggable={false} className="absolute inset-0 h-full w-full object-cover" />
            ) : (
              <Art title={song.album} className="absolute inset-0 h-full w-full" big />
            )}
            <div
              className="absolute inset-0"
              style={{ background: 'linear-gradient(to bottom, transparent 35%, color-mix(in oklab, var(--solid) 92%, transparent) 100%)' }}
            />
            <AnimatePresence>
              {showLyrics && lyrics?.lines.length ? (
                <motion.div
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  className="absolute inset-x-0 bottom-0 px-4 pb-2"
                >
                  {[at - 1, at, at + 1].map((i) =>
                    i >= 0 && i < lyrics.lines.length ? (
                      <div
                        key={i}
                        className={`truncate text-[15px] font-semibold leading-snug transition-colors duration-300 ${i === at ? 'text-ink' : 'text-ink/45'}`}
                      >
                        {lyrics.lines[i][1] || '♪'}
                      </div>
                    ) : (
                      <div key={i} className="h-[1.375em] text-[15px]" />
                    ),
                  )}
                </motion.div>
              ) : null}
            </AnimatePresence>
          </div>
          <div className="relative bg-[color-mix(in_oklab,var(--solid)_92%,transparent)] px-4 pb-3.5 pt-1">
            <div className="truncate text-[15px] font-semibold">{song.title}</div>
            <div className="truncate text-[12.5px] text-ink/60">{artist}</div>
            <Seek value={progress} duration={duration} position={s.position_ms} />
            <div className="mt-1 flex justify-center">
              <Controls playing={s.playing} />
            </div>
          </div>
          <WindowButtons
            shape={shape}
            nextShape={nextShape}
            onShape={resize}
            lyrics={song.lyrics ? { on: lyricsOn, toggle: () => setLyricsOn((o) => !o) } : null}
            onArt
          />
        </div>
      )}
    </div>
  )
}

/** The window's own light: the cover, blurred, under a scrim (or the accent, without one). */
function Light({ src, card }: { src?: string; card: boolean }) {
  return (
    <div aria-hidden className="absolute inset-0">
      <div className="absolute inset-0 bg-[color-mix(in_oklab,var(--solid)_72%,transparent)]" />
      {src && !card && (
        <img src={src} alt="" className="absolute inset-0 h-full w-full scale-150 object-cover opacity-60 blur-2xl saturate-150" />
      )}
      <div
        className="absolute inset-0"
        style={{
          background: src
            ? 'color-mix(in oklab, var(--solid) 45%, transparent)'
            : 'radial-gradient(120% 140% at 0% 0%, color-mix(in oklab, var(--accent) 30%, transparent), transparent 70%)',
        }}
      />
    </div>
  )
}

/** The cover, or a tile in the accent with a note when there is none. */
function Art({ src, title, className = '', big = false }: { src?: string; title: string; className?: string; big?: boolean }) {
  return (
    <div className={`relative shrink-0 overflow-hidden bg-ink/[0.08] shadow-[0_8px_24px_-10px_rgb(0_0_0/0.6)] ${className}`}>
      {src ? (
        <img src={src} alt="" draggable={false} className="h-full w-full object-cover" />
      ) : (
        <div
          title={title}
          className={`grid h-full w-full place-items-center text-ink/70 ${big ? 'text-[64px]' : 'text-[20px]'}`}
          style={{ background: 'linear-gradient(135deg, color-mix(in oklab, var(--accent) 55%, transparent), color-mix(in oklab, var(--accent) 12%, transparent))' }}
        >
          ♪
        </div>
      )}
    </div>
  )
}

function PlayButton({ playing, size }: { playing: boolean; size: number }) {
  return (
    <motion.button
      whileTap={{ scale: 0.9 }}
      onClick={() => void toggle()}
      aria-label={playing ? 'Pause' : 'Play'}
      title={playing ? 'Pause (Space)' : 'Play (Space)'}
      className="grid shrink-0 place-items-center rounded-full bg-accent text-on-accent shadow-md"
      style={{ width: size, height: size }}
    >
      {playing ? <IconPause width={16} height={16} /> : <IconPlay width={16} height={16} />}
    </motion.button>
  )
}

function Controls({ playing }: { playing: boolean }) {
  const small = 'grid h-8 w-8 place-items-center rounded-full text-ink/75 transition-colors hover:bg-ink/10 hover:text-ink'
  return (
    <div className="flex items-center gap-1.5">
      <button onClick={() => void previous()} aria-label="Previous" title="Previous (Ctrl ←)" className={small}>
        <IconPrevious width={17} height={17} />
      </button>
      <PlayButton playing={playing} size={38} />
      <button onClick={() => void next()} aria-label="Next" title="Next (Ctrl →)" className={small}>
        <IconNext width={17} height={17} />
      </button>
    </div>
  )
}

/** A thin line along the bottom edge; click to jump. */
function Progress({ value, duration = 0 }: { value: number; duration?: number }) {
  const bar = useRef<HTMLDivElement>(null)
  return (
    <div
      ref={bar}
      data-no-drag
      onClick={(e) => {
        const r = bar.current?.getBoundingClientRect()
        if (r && duration) void seek(((e.clientX - r.left) / r.width) * duration)
      }}
      className={`absolute inset-x-0 bottom-0 h-[3px] bg-ink/10 ${duration ? 'cursor-pointer hover:h-[5px]' : ''} transition-[height]`}
    >
      <div className="h-full bg-accent transition-[width] duration-200 ease-linear" style={{ width: `${value * 100}%` }} />
    </div>
  )
}

function Seek({ value, duration, position }: { value: number; duration: number; position: number }) {
  const bar = useRef<HTMLDivElement>(null)
  return (
    <div className="mt-2 flex items-center gap-2 text-[10.5px] tabular-nums text-ink/55">
      <span className="w-8">{clock(position)}</span>
      <div
        ref={bar}
        data-no-drag
        onClick={(e) => {
          const r = bar.current?.getBoundingClientRect()
          if (r && duration) void seek(((e.clientX - r.left) / r.width) * duration)
        }}
        className="group/seek relative h-4 flex-1 cursor-pointer"
      >
        <div className="absolute inset-x-0 top-1/2 h-[3px] -translate-y-1/2 rounded-full bg-ink/15">
          <div className="h-full rounded-full bg-accent" style={{ width: `${value * 100}%` }} />
        </div>
      </div>
      <span className="w-8 text-right">{clock(duration)}</span>
    </div>
  )
}

/** Top right, shown on hover: lyrics, change size, back to the full app. */
function WindowButtons(props: {
  shape: Shape
  nextShape: Shape
  onShape: (s: Shape) => void
  lyrics: { on: boolean; toggle: () => void } | null
  compact?: boolean
  onArt?: boolean
}) {
  const btn = `grid h-7 w-7 place-items-center rounded-full transition-colors ${props.onArt ? 'on-art' : 'text-ink/70 hover:bg-ink/10 hover:text-ink'}`
  const names: Record<Shape, string> = { micro: 'Smallest', mini: 'Bar', card: 'Card' }
  const icon = useMemo(
    () => (
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" aria-hidden>
        {props.nextShape === 'card' ? <rect x="6" y="3.5" width="12" height="17" rx="2.5" /> : props.nextShape === 'mini' ? <rect x="3" y="7" width="18" height="10" rx="2.5" /> : <rect x="3" y="9.5" width="18" height="5" rx="2.5" />}
      </svg>
    ),
    [props.nextShape],
  )
  return (
    <div
      className={`flex items-center gap-1 transition-opacity duration-200 ${
        props.compact ? '' : 'absolute right-2 top-2 opacity-0 group-hover/mini:opacity-100 focus-within:opacity-100'
      }`}
    >
      {props.lyrics && (
        <button
          onClick={props.lyrics.toggle}
          aria-pressed={props.lyrics.on}
          aria-label="Lyrics"
          title={props.lyrics.on ? 'Hide lyrics' : 'Show lyrics'}
          className={`${btn} ${props.lyrics.on ? '!text-accent-ink' : ''}`}
        >
          <IconLyrics width={15} height={15} />
        </button>
      )}
      <button onClick={() => props.onShape(props.nextShape)} aria-label={`Size: ${names[props.nextShape]}`} title={`Size: ${names[props.nextShape]}`} className={btn}>
        {icon}
      </button>
      <button onClick={() => void miniPlayer(false)} aria-label="Back to Venuta" title="Back to Venuta" className={btn}>
        <IconExpand width={14} height={14} />
      </button>
    </div>
  )
}
