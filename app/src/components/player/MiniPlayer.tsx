import { currentMonitor, getCurrentWindow, LogicalSize, PhysicalPosition } from '@tauri-apps/api/window'
import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { loadCovers, setPlayingAccent, useCovers } from '../../lib/covers'
import { inApp } from '../../lib/engine'
import { clock, songLength, connectPlayer, current, isOnline, jumpTo, miniPlayer, next, previous, seek, setRepeat, setShuffle, setVolume, toggle, usePlayer, type PlayerState } from '../../lib/player'
import { canSeekNow } from '../../lib/streamWait'
import { LikeButton } from '../LikeButton'
import { artUrl, OnlineCover } from '../RemoteArt'
import { SteadyArt } from '../SteadyArt'
import { IconExpand, IconLyrics, IconMute, IconNext, IconPause, IconPlay, IconPrevious, IconQueue, IconRepeat, IconRepeatOne, IconShuffle, IconVolume } from '../Icons'
import { lineAt, useSongLyrics } from '../../lib/lyrics'
import { keyAction } from '../../lib/secondChoice'
import { useSecondLines, rowViewsAt, type SecondLinesView } from '../../lib/useSecondLines'
import type { SecondRowView } from '../lyrics/LyricText'
import { SECOND_NAMES } from '../../lib/secondChoice'
import { useShrinkToFit } from '../lyrics/useShrinkToFit'
import { ThemedTrack } from './NowPlayingBar'
import { LcdSpectrum } from './Spectrum'
import { useThemeInfo } from '../../lib/prefs'
import type { SeekBar } from '../../lib/themes'

/*
  The mini player: its own small window that stays on top. One window, three shapes, chosen by
  its size, so dragging an edge is enough to change it:
    micro  a strip (cover, title, play)
    mini   a bar (cover, title and artist or the line being sung, controls, a seek line)
    card   tall (the cover fills it; title and controls over it; a few lines of lyrics)
  Without a cover it shows a tile in the accent; without lyrics there is no lyrics button.
  It stays on top unless unpinned, settles against a screen edge when dropped near one, and
  fades a little when left alone.
*/
type Shape = 'micro' | 'mini' | 'card'
const SIZES: Record<Shape, [number, number]> = { micro: [320, 64], mini: [380, 132], card: [300, 400] }
const shapeOf = (w: number, h: number): Shape => (h < 96 ? 'micro' : h >= 220 && w < h * 1.4 ? 'card' : 'mini')

const KEY = 'mini-player'
type Saved = { lyrics: boolean; onTop: boolean }
const saved = (): Saved => {
  try {
    return { lyrics: false, onTop: true, ...JSON.parse(localStorage.getItem(KEY) ?? '{}') }
  } catch {
    return { lyrics: false, onTop: true }
  }
}

/** Dropped within reach of a screen edge (clear of the taskbar), the window settles against it. */
function useSnapToEdges() {
  useEffect(() => {
    if (!inApp()) return
    const win = getCurrentWindow()
    let timer = 0
    let off: (() => void) | undefined
    let live = true
    const snap = async () => {
      const [mon, pos, size] = await Promise.all([currentMonitor(), win.outerPosition(), win.outerSize()])
      if (!mon) return
      const k = mon.scaleFactor
      const gap = Math.round(12 * k)
      const reach = Math.round(32 * k)
      const a = mon.workArea
      const [left, top] = [a.position.x, a.position.y]
      const [right, bottom] = [left + a.size.width, top + a.size.height]
      let { x, y } = pos
      if (Math.abs(x - left) < reach) x = left + gap
      else if (Math.abs(right - (x + size.width)) < reach) x = right - size.width - gap
      if (Math.abs(y - top) < reach) y = top + gap
      else if (Math.abs(bottom - (y + size.height)) < reach) y = bottom - size.height - gap
      if (x !== pos.x || y !== pos.y) await win.setPosition(new PhysicalPosition(x, y))
    }
    void win
      .onMoved(() => {
        window.clearTimeout(timer)
        timer = window.setTimeout(() => void snap().catch(() => {}), 450)
      })
      .then((u) => (live ? (off = u) : u()))
    return () => {
      live = false
      window.clearTimeout(timer)
      off?.()
    }
  }, [])
}

/**
 * Change shape without leaving the screen: a window against the right or bottom edge grows
 * towards the middle, and the result is kept inside the work area.
 */
async function reshape(to: Shape) {
  const win = getCurrentWindow()
  const [mon, pos, size] = await Promise.all([currentMonitor(), win.outerPosition(), win.outerSize()])
  await win.setSize(new LogicalSize(...SIZES[to]))
  if (!mon) return
  const after = await win.outerSize()
  const a = mon.workArea
  const reach = Math.round(32 * mon.scaleFactor)
  const [left, top] = [a.position.x, a.position.y]
  const [right, bottom] = [left + a.size.width, top + a.size.height]
  let x = pos.x
  let y = pos.y
  if (right - (pos.x + size.width) < reach) x = pos.x + size.width - after.width
  if (bottom - (pos.y + size.height) < reach) y = pos.y + size.height - after.height
  x = Math.max(left, Math.min(x, right - after.width))
  y = Math.max(top, Math.min(y, bottom - after.height))
  if (x !== pos.x || y !== pos.y) await win.setPosition(new PhysicalPosition(x, y))
}

/** True a few seconds after the pointer leaves the window (or never came in). */
function useIdle(ms = 3000) {
  const [idle, setIdle] = useState(false)
  const timer = useRef(0)
  useEffect(() => {
    const away = () => {
      window.clearTimeout(timer.current)
      timer.current = window.setTimeout(() => setIdle(true), ms)
    }
    const back = () => {
      window.clearTimeout(timer.current)
      setIdle(false)
    }
    away()
    document.addEventListener('mouseenter', back)
    document.addEventListener('mousemove', back)
    document.addEventListener('mouseleave', away)
    window.addEventListener('blur', away)
    return () => {
      window.clearTimeout(timer.current)
      document.removeEventListener('mouseenter', back)
      document.removeEventListener('mousemove', back)
      document.removeEventListener('mouseleave', away)
      window.removeEventListener('blur', away)
    }
  }, [ms])
  return idle
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
  // A retro theme dresses the mini player in its own player bar, seek bar and type.
  const info = useThemeInfo()
  const retro = info.miniPlayer === 'themed'
  const { w, h } = useWindowSize()
  const shape = shapeOf(w, h)
  const [lyricsOn, setLyricsOn] = useState(saved().lyrics)
  const [onTop, setOnTop] = useState(saved().onTop)
  const [queueOn, setQueueOn] = useState(false)
  useEffect(() => {
    try {
      localStorage.setItem(KEY, JSON.stringify({ lyrics: lyricsOn, onTop } satisfies Saved))
    } catch {
      /* a convenience only */
    }
  }, [lyricsOn, onTop])
  useEffect(() => {
    if (inApp()) void getCurrentWindow().setAlwaysOnTop(onTop).catch(() => {})
  }, [onTop])
  useSnapToEdges()
  // Left alone, it steps back a little (never so far that it can't be read).
  const idle = useIdle()
  const pin = { on: onTop, toggle: () => setOnTop((o) => !o) }

  // This window has its own cover cache: fetch the playing album's.
  const cover = useCovers(320)
  const online = !!song && isOnline(song)
  // An online song has no album folder: its picture comes from the catalog.
  const dir = online ? '' : (song?.dir ?? '')
  useEffect(() => {
    if (root && dir) void loadCovers(root, [dir])
  }, [root, dir])
  const remoteSrc = online ? artUrl(song?.artwork_url, 360) : undefined
  const art = online ? (remoteSrc ? { src: remoteSrc, accent: undefined } : undefined) : dir ? cover(dir) : undefined
  useEffect(() => setPlayingAccent(art?.accent), [art?.accent])

  // Your file's words, else the lyrics services' (online songs too).
  const showLyrics = lyricsOn && !!song && shape !== 'micro'
  const lyrics = useSongLyrics(root, song ?? undefined, showLyrics)
  const at = lineAt(lyrics, s.position_ms)
  const line = at >= 0 ? lyrics?.lines[at]?.[1] || '♪' : ''
  // Its pronunciation and translation, when switched on: the same rows for every line.
  const seconds = useSecondLines(song, lyrics)
  const rowsOn = showLyrics && (seconds.rows.roman || seconds.rows.translation)
  const rows = rowsOn ? (rowViewsAt(seconds, at) ?? []) : []
  const toggleRow = useRef(seconds.toggle)
  useEffect(() => {
    toggleRow.current = (kind) => showLyrics && seconds.offered[kind] && seconds.toggle(kind)
  }, [seconds, showLyrics])

  // Space plays and pauses; arrows skip; Ctrl+M goes back.
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      const row = keyAction(e)
      if (row) toggleRow.current(row)
      else if (e.key === ' ') {
        e.preventDefault()
        void toggle()
      } else if (e.key === 'ArrowRight' && e.ctrlKey) void next()
      else if (e.key === 'ArrowLeft' && e.ctrlKey) void previous()
      // Ctrl+M again: back to the main window (as the button says).
      else if (e.ctrlKey && (e.key === 'm' || e.key === 'M')) {
        e.preventDefault()
        void miniPlayer(false)
      }
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
  const resize = (to: Shape) => inApp() && void reshape(to).catch(() => {})
  const nextShape: Shape = shape === 'micro' ? 'mini' : shape === 'mini' ? 'card' : 'micro'

  const artist = song ? song.artists.join(', ') || song.album_artist : ''
  const duration = s.duration_ms || song?.duration_ms || 0
  const progress = duration ? Math.min(1, s.position_ms / duration) : 0
  // While an online song is still opening there is nothing to seek in yet.
  const seekable = canSeekNow(s) ? duration : 0

  return (
    <div
      onMouseDown={drag}
      className="group/mini relative h-full w-full select-none overflow-hidden text-ink transition-opacity duration-500"
      style={{ opacity: idle ? 0.85 : 1 }}
    >
      {retro ? <div aria-hidden className="np absolute inset-0 !rounded-none" /> : <Light src={art?.src} card={shape === 'card'} />}

      {!song ? (
        <div className="relative grid h-full place-items-center px-4 text-center text-[13px] text-ink/60">
          Nothing playing. Choose music in Venuta.
          <WindowButtons shape={shape} nextShape={nextShape} onShape={resize} lyrics={null} pin={pin} />
        </div>
      ) : shape === 'micro' ? (
        <div className="relative flex h-full items-center gap-3 pl-2 pr-2">
          <Art src={art?.src} title={song.album} seed={online ? song.album || song.title : undefined} className="h-11 w-11 rounded-[10px]" />
          <div className="min-w-0 flex-1">
            <div className="truncate text-[13px] font-semibold leading-tight">{song.title}</div>
            <div className="truncate text-[11.5px] leading-tight text-ink/60">{artist}</div>
          </div>
          <PlayButton playing={s.playing} size={36} />
          <WindowButtons shape={shape} nextShape={nextShape} onShape={resize} lyrics={null} pin={pin} compact />
          {retro ? <RetroTrack bar={info.seekBar} value={progress} /> : <Progress value={progress} />}
        </div>
      ) : shape === 'mini' ? (
        <div className="relative flex h-full items-center gap-3.5 p-3 pb-4">
          <Art src={art?.src} title={song.album} seed={online ? song.album || song.title : undefined} className="aspect-square h-full rounded-[14px]" />
          <div className="flex min-w-0 flex-1 flex-col justify-center gap-1.5 self-stretch">
            <div className={`min-w-0 pr-16 ${rowsOn ? 'flex min-h-0 flex-1 flex-col' : ''}`}>
              <div className={`truncate text-[14.5px] font-semibold leading-tight ${retro ? 'title' : ''}`}>{song.title}</div>
              {rowsOn ? (
                // The line and the rows under it, wrapped, made smaller rather than cut off.
                <FittedLines lines={[{ text: line || '♪', now: true, rows }]} size={12.5} lang={seconds.lang} className="mt-0.5" />
              ) : (
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
              )}
            </div>
            <Controls playing={s.playing} />
          </div>
          {info.lcd && <LcdSpectrum className="h-full w-[92px] shrink-0" segments={12} />}
          <WindowButtons
            shape={shape}
            nextShape={nextShape}
            onShape={resize}
            lyrics={{ on: lyricsOn, toggle: () => setLyricsOn((o) => !o) }}
            seconds={showLyrics ? seconds : null}
            pin={pin}
          />
          {retro ? <RetroTrack bar={info.seekBar} value={progress} /> : <Progress value={progress} duration={seekable} />}
        </div>
      ) : (
        <div className="relative flex h-full flex-col">
          {/* The cover fills the top; words and controls sit on a fade at the bottom. */}
          <div className="relative min-h-0 flex-1">
            {queueOn ? (
              <UpNext s={s} />
            ) : (
              <Art src={art?.src} title={song.album} seed={online ? song.album || song.title : undefined} className="absolute inset-0 h-full w-full" big />
            )}
            <div
              className="absolute inset-0"
              style={{ background: 'linear-gradient(to bottom, transparent 35%, color-mix(in oklab, var(--solid) 92%, transparent) 100%)' }}
            />
            <AnimatePresence>
              {rowsOn && !queueOn && lyrics?.lines.length ? (
                // With pronunciation or translation: the lines wrap, the sung one with its rows
                // under it, all made smaller as much as they must to fit over the cover.
                <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} className="absolute inset-x-0 bottom-0 top-[22%] flex flex-col justify-end px-4 pb-2">
                  <FittedLines
                    size={15}
                    lang={seconds.lang}
                    bottom
                    lines={[
                      { text: at > 0 ? lyrics.lines[at - 1][1] || '♪' : '', now: false, rows: [] },
                      { text: at >= 0 ? lyrics.lines[at][1] || '♪' : '', now: true, rows },
                      { text: lyrics.lines[at + 1]?.[1] ?? '', now: false, rows: [] },
                    ]}
                  />
                </motion.div>
              ) : showLyrics && !queueOn && lyrics?.lines.length ? (
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
            <div className={`truncate text-[15px] font-semibold ${retro ? 'title' : ''}`}>{song.title}</div>
            <div className="truncate text-[12.5px] text-ink/60">{artist}</div>
            {retro ? (
              <div className="relative mt-3 h-5">
                <ThemedTrack bar={info.seekBar} pct={progress * 100} />
              </div>
            ) : (
              <Seek value={progress} duration={duration} position={s.position_ms} canSeek={canSeekNow(s)} />
            )}
            <div className="mt-1 flex items-center justify-between">
              {s.root ? <LikeButton root={s.root} item={song} size={16} /> : <span className="w-8" />}
              <button onClick={() => void setShuffle(!s.shuffle)} aria-pressed={s.shuffle} aria-label="Shuffle" title={s.shuffle ? 'Shuffle is on' : 'Shuffle'} className={`${smallButton} ${s.shuffle ? '!text-accent-ink' : ''}`}>
                <IconShuffle width={15} height={15} />
              </button>
              <Controls playing={s.playing} />
              <button
                onClick={() => void setRepeat(s.repeat === 'off' ? 'all' : s.repeat === 'all' ? 'one' : 'off')}
                aria-label={s.repeat === 'off' ? 'Repeat' : s.repeat === 'all' ? 'Repeat all' : 'Repeat this song'}
                title={s.repeat === 'off' ? 'Repeat' : s.repeat === 'all' ? 'Repeating all' : 'Repeating this song'}
                className={`${smallButton} ${s.repeat !== 'off' ? '!text-accent-ink' : ''}`}
              >
                {s.repeat === 'one' ? <IconRepeatOne width={15} height={15} /> : <IconRepeat width={15} height={15} />}
              </button>
              <MuteButton volume={s.volume} />
            </div>
          </div>
          <WindowButtons
            shape={shape}
            nextShape={nextShape}
            onShape={resize}
            lyrics={!queueOn ? { on: lyricsOn, toggle: () => setLyricsOn((o) => !o) } : null}
            seconds={showLyrics && !queueOn ? seconds : null}
            queue={{ on: queueOn, toggle: () => setQueueOn((o) => !o) }}
            pin={pin}
            onArt={!queueOn}
          />
        </div>
      )}
    </div>
  )
}

/** The card's "Up next": the songs after this one; click one to play it. */
function UpNext({ s }: { s: PlayerState }) {
  const after = s.queue.slice(s.index + 1, s.index + 30).map((q, k) => ({ q, i: s.index + 1 + k }))
  return (
    <div data-no-drag className="thin-scroll absolute inset-0 overflow-y-auto bg-[color-mix(in_oklab,var(--solid)_92%,transparent)] px-2 pb-6 pt-11">
      <div className="px-2 pb-1.5 text-[11.5px] font-medium text-ink/55">Up next</div>
      {after.length === 0 ? (
        <p className="px-2 py-6 text-center text-[12.5px] text-ink/50">Nothing after this song.</p>
      ) : (
        after.map(({ q, i }) => (
          <button
            key={`${q.path}#${i}`}
            onClick={() => void jumpTo(i)}
            className="block w-full rounded-lg px-2 py-1.5 text-left transition-colors hover:bg-ink/[0.07] active:bg-ink/[0.11]"
          >
            <span className="block truncate text-[13px]">{q.title}</span>
            <span className="block truncate text-[11.5px] text-ink/55">{q.artists.join(', ') || q.album_artist}</span>
          </button>
        ))
      )}
    </div>
  )
}

/** A retro theme's own seek bar, along the bottom edge. */
function RetroTrack({ bar, value }: { bar: SeekBar; value: number }) {
  return (
    <div className="absolute inset-x-2 bottom-1 h-4">
      <ThemedTrack bar={bar} pct={value * 100} />
    </div>
  )
}

/** The window's own light: the cover, blurred, under a scrim (or the accent, without one). */
function Light({ src: given, card }: { src?: string; card: boolean }) {
  const [failed, setFailed] = useState<string | null>(null)
  const src = failed === given ? undefined : given
  return (
    <div aria-hidden className="absolute inset-0">
      <div className="absolute inset-0 bg-[color-mix(in_oklab,var(--solid)_72%,transparent)]" />
      {src && !card && (
        <img src={src} alt="" onError={() => setFailed(src ?? null)} className="absolute inset-0 h-full w-full scale-150 object-cover opacity-60 blur-2xl saturate-150" />
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

/**
 * The cover, held steady across a change of song (components/SteadyArt), or a tile in the accent
 * with a note when there is none; an online song without a picture gets the online cover.
 */
function Art({ src, title, seed, className = '', big = false }: { src?: string; title: string; seed?: string; className?: string; big?: boolean }) {
  const tile = seed !== undefined ? (
    <OnlineCover seed={seed} />
  ) : (
    <div
      title={title}
      className={`grid h-full w-full place-items-center text-ink/70 ${big ? 'text-[64px]' : 'text-[20px]'}`}
      style={{ background: 'linear-gradient(135deg, color-mix(in oklab, var(--accent) 55%, transparent), color-mix(in oklab, var(--accent) 12%, transparent))' }}
    >
      ♪
    </div>
  )
  return (
    <div className={`shrink-0 overflow-hidden bg-ink/[0.08] shadow-[0_8px_24px_-10px_rgb(0_0_0/0.6)] ${className}`}>
      <SteadyArt src={src} fallback={tile} />
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
      className="np-play accent-fill grid shrink-0 place-items-center rounded-full text-on-accent"
      style={{ width: size, height: size }}
    >
      {playing ? <IconPause width={16} height={16} /> : <IconPlay width={16} height={16} />}
    </motion.button>
  )
}

const smallButton = 'grid h-8 w-8 shrink-0 place-items-center rounded-full text-ink/70 transition-colors hover:bg-ink/10 hover:text-ink'

/** Mute and back to the volume it had. */
function MuteButton({ volume }: { volume: number }) {
  const before = useRef(volume || 1)
  useEffect(() => {
    if (volume > 0) before.current = volume
  }, [volume])
  const muted = volume === 0
  return (
    <button onClick={() => void setVolume(muted ? before.current : 0)} aria-pressed={muted} aria-label={muted ? 'Unmute' : 'Mute'} title={muted ? 'Unmute' : 'Mute'} className={smallButton}>
      {muted ? <IconMute width={15} height={15} /> : <IconVolume width={15} height={15} />}
    </button>
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

function Seek({ value, duration, position, canSeek }: { value: number; duration: number; position: number; canSeek: boolean }) {
  const bar = useRef<HTMLDivElement>(null)
  return (
    <div className="mt-2 flex items-center gap-2 text-[10.5px] tabular-nums text-ink/55">
      <span className="w-8">{clock(position)}</span>
      <div
        ref={bar}
        data-no-drag
        onClick={(e) => {
          const r = bar.current?.getBoundingClientRect()
          if (r && duration && canSeek) void seek(((e.clientX - r.left) / r.width) * duration)
        }}
        title={canSeek ? undefined : 'Opening…'}
        className={`group/seek relative h-4 flex-1 ${canSeek ? 'cursor-pointer' : 'cursor-progress opacity-60'}`}
      >
        <div className="absolute inset-x-0 top-1/2 h-[3px] -translate-y-1/2 rounded-full bg-ink/15">
          <div className="h-full rounded-full bg-accent" style={{ width: `${value * 100}%` }} />
        </div>
      </div>
      <span className="w-8 text-right">{songLength(duration)}</span>
    </div>
  )
}

/** Top right, shown on hover: lyrics, change size, back to the full app. */
function WindowButtons(props: {
  shape: Shape
  nextShape: Shape
  onShape: (s: Shape) => void
  lyrics: { on: boolean; toggle: () => void } | null
  /** The song's two switches, while its lyrics show. */
  seconds?: SecondLinesView | null
  queue?: { on: boolean; toggle: () => void }
  pin: { on: boolean; toggle: () => void }
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
      {(['roman', 'translation'] as const)
        .filter((k) => props.seconds?.offered[k])
        .map((k) => {
          const on = !!props.seconds?.on[k]
          const note = k === 'roman' ? (props.seconds?.notes.roman ?? props.seconds?.dictionary) : props.seconds?.notes.translation
          return (
            <button
              key={k}
              onClick={() => props.seconds?.toggle(k)}
              aria-pressed={on}
              aria-label={SECOND_NAMES[k].name}
              title={`${SECOND_NAMES[k].name}: ${SECOND_NAMES[k].line.toLowerCase()} (${SECOND_NAMES[k].keys})${note ? `. ${note}` : ''}`}
              className={`${btn} text-[11px] font-semibold ${on ? '!text-accent-ink' : ''}`}
            >
              {/* A for the words in English letters; a speech mark for their meaning. */}
              {k === 'roman' ? (
                <span aria-hidden>Ab</span>
              ) : (
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
                  <path d="M4 5h9M8.5 3v2M6 5c.5 3 2.5 5.5 5.5 7M11 5c-.8 3.5-3.2 6.5-6.5 8" />
                  <path d="m13 21 4-9 4 9M14.5 18h5" />
                </svg>
              )}
            </button>
          )
        })}
      {props.queue && (
        <button
          onClick={props.queue.toggle}
          aria-pressed={props.queue.on}
          aria-label="Up next"
          title={props.queue.on ? 'Hide up next' : 'Up next'}
          className={`${btn} ${props.queue.on ? '!text-accent-ink' : ''}`}
        >
          <IconQueue width={15} height={15} />
        </button>
      )}
      {!props.compact && (
        <button
          onClick={props.pin.toggle}
          aria-pressed={props.pin.on}
          aria-label="Keep on top"
          title={props.pin.on ? 'Kept on top of other windows (click to let them cover it)' : 'Keep on top of other windows'}
          className={`${btn} ${props.pin.on ? '' : 'opacity-60'}`}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill={props.pin.on ? 'currentColor' : 'none'} stroke="currentColor" strokeWidth="1.9" strokeLinejoin="round" strokeLinecap="round" aria-hidden>
            <path d="M9 3.5h6l-1 5.5 3.5 3.5h-11L10 9z" />
            <path d="M12 12.5v8" fill="none" />
          </svg>
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

/**
 * Lines in a slot of fixed height (the bar's one line, the card's three over the cover), each
 * with its rows under it (pronunciation, then translation, smaller and dimmer): wrapped, never
 * cut off, made smaller as much as they must to fit.
 */
function FittedLines(props: { lines: { text: string; now: boolean; rows: SecondRowView[] }[]; size: number; lang: string | null; bottom?: boolean; className?: string }) {
  const key = props.lines.map((l) => `${l.text}|${l.rows.map((r) => r.text).join('|')}`).join('/')
  const { box, inner, fit } = useShrinkToFit<HTMLDivElement, HTMLDivElement>(key)
  return (
    <div ref={box} className={`flex min-h-0 flex-1 flex-col overflow-hidden ${props.bottom ? 'justify-end' : 'justify-start'} ${props.className ?? ''}`}>
      <div ref={inner} style={{ fontSize: props.size * fit, overflowWrap: 'anywhere' }} className="grid gap-0.5">
        {props.lines.map((l, i) => (
          <div key={i} className={l.now ? '' : 'text-ink/45'}>
            <div lang={props.lang ?? undefined} className={`font-semibold leading-snug ${l.now ? 'text-accent-ink' : ''}`}>
              {l.text || '\u00a0'}
            </div>
            {l.rows.map((r) => (
              <div key={r.kind} lang={r.lang} className={`leading-snug ${r.kind === 'roman' ? 'text-ink/70' : 'text-ink/55'}`} style={{ fontSize: r.kind === 'roman' ? '0.86em' : '0.8em' }}>
                {r.text || '\u00a0'}
              </div>
            ))}
          </div>
        ))}
      </div>
    </div>
  )
}
