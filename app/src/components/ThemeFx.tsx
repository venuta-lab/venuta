import { useEffect, useMemo, useRef, useState } from 'react'
import { reducedMotion, usePrefs } from '../lib/prefs'
import { current, usePlayerValue } from '../lib/player'

/*
  The retro themes' atmosphere, in two fixed layers that never take a click:
    under   behind the content (watermarks, data rain, grids, wires, halftones)
    over    above everything, very faint (grain, scanlines, static)
  Everything moving is a transform on a pre-drawn layer, so the compositor does the work; with
  reduced motion it all holds still. The track list itself is never decorated.
*/
const GRAIN =
  "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='160' height='160'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='.9' numOctaves='2' stitchTiles='stitch'/%3E%3CfeColorMatrix values='0 0 0 0 .5 0 0 0 0 .5 0 0 0 0 .5 0 0 0 1 0'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23n)'/%3E%3C/svg%3E\")"
const SCANLINES = 'repeating-linear-gradient(0deg, rgb(0 0 0 / 0.18) 0 1px, transparent 1px 3px)'
const HEX =
  "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='56' height='97'%3E%3Cpath d='M28 0 56 16v32L28 64 0 48V16zM28 64v33' fill='none' stroke='%23f66e25' stroke-width='1'/%3E%3C/svg%3E\")"

export function ThemeFx() {
  const p = usePrefs()
  const reduced = reducedMotion(p)
  switch (p.style) {
    case 'neo-tokyo':
      return (
        <>
          <Under>
            {/* A watermark of a plain word (音楽, "music"), set vertically down the right edge. */}
            <div
              className="absolute right-[2vw] top-[8vh] select-none text-[24vh] leading-none text-[#f2ebdd]"
              style={{ fontFamily: "\"Dela Gothic One\", sans-serif", writingMode: "vertical-rl", opacity: 0.028 }}
            >
              音楽再生
            </div>
          </Under>
          <Over style={{ backgroundImage: GRAIN, opacity: 0.05, mixBlendMode: 'overlay' }} />
          {!reduced && <LightTrails />}
        </>
      )
    case 'thermoptic':
      return (
        <>
          <Under>{!reduced ? <DataRain /> : <DataRain still />}</Under>
          <Over style={{ backgroundImage: SCANLINES, opacity: 0.3 }} />
        </>
      )
    case 'sync-ratio':
      return (
        <>
          <Under style={{ backgroundImage: HEX, backgroundSize: '56px 97px', opacity: 0.045 }} />
          <Over style={{ backgroundImage: SCANLINES, opacity: 0.18 }} />
        </>
      )
    case 'layer-zero':
      return (
        <>
          <Under>
            <Wires />
          </Under>
          <Over
            className={reduced ? '' : 'fx-static'}
            style={{ backgroundImage: GRAIN, opacity: 0.035, mixBlendMode: 'screen' }}
          />
          {!reduced && <RgbSplit />}
        </>
      )
    case 'bounty':
      return (
        <>
          <Under>
            <div
              className="absolute right-0 top-0 h-[38vh] w-[38vw]"
              style={{
                background: 'radial-gradient(var(--ink) 1px, transparent 1.3px) 0 0 / 7px 7px',
                maskImage: 'radial-gradient(100% 100% at 100% 0%, #000, transparent 70%)',
                opacity: 0.16,
              }}
            />
            <div
              className="absolute bottom-0 left-0 h-[30vh] w-[30vw]"
              style={{
                background: 'radial-gradient(var(--accent) 1.2px, transparent 1.5px) 0 0 / 8px 8px',
                maskImage: 'radial-gradient(100% 100% at 0% 100%, #000, transparent 70%)',
                opacity: 0.25,
              }}
            />
          </Under>
          <Over style={{ backgroundImage: GRAIN, opacity: 0.07, mixBlendMode: 'multiply' }} />
          {!reduced && <BlockWipe />}
        </>
      )
    case 'skin99':
      return (
        <Under
          style={{
            backgroundImage: 'repeating-linear-gradient(90deg, rgb(255 255 255 / 0.025) 0 1px, transparent 1px 3px)',
          }}
        />
      )
    default:
      return null
  }
}

function Under(props: { children?: React.ReactNode; style?: React.CSSProperties }) {
  return (
    <div aria-hidden className="pointer-events-none fixed inset-0 z-[1] overflow-hidden" style={props.style}>
      {props.children}
    </div>
  )
}

function Over(props: { style?: React.CSSProperties; className?: string }) {
  return <div aria-hidden className={`pointer-events-none fixed inset-0 z-[90] ${props.className ?? ''}`} style={props.style} />
}

/** Neo-Tokyo: red light-trails sweep across when the song changes. */
function LightTrails() {
  // Each new song replays the trails (by key).
  const path = useSongChange()
  if (!path) return null
  return (
    <div key={path} aria-hidden className="pointer-events-none fixed inset-0 z-[80] overflow-hidden">
      {[22, 58, 81].map((top, i) => (
        <span
          key={top}
          className="fx-trail absolute left-0 h-[2px] w-[60vw]"
          style={{
            top: `${top}%`,
            animationDelay: `${i * 70}ms`,
            background: 'linear-gradient(90deg, transparent, #ff2a2a 70%, #ffd0c0)',
            filter: 'blur(1px)',
            boxShadow: '0 0 12px #ff2a2a',
          }}
        />
      ))}
    </div>
  )
}

/** The song playing, once it changes after the theme appeared (not for the one already on). */
function useSongChange(): string {
  const path = usePlayerValue((s) => current(s)?.path ?? '')
  const [first] = useState(path)
  return path && path !== first ? path : ''
}

/** Layer Zero: a 1px RGB split over the page for 120ms when the song changes. */
function RgbSplit() {
  const path = useSongChange()
  const timer = useRef(0)
  useEffect(() => {
    if (!path) return
    const main = document.querySelector('main')
    main?.classList.add('fx-split')
    window.clearTimeout(timer.current)
    timer.current = window.setTimeout(() => main?.classList.remove('fx-split'), 120)
    return () => {
      window.clearTimeout(timer.current)
      main?.classList.remove('fx-split')
    }
  }, [path])
  return null
}

/** Bounty Session: the four block colours wipe across the player bar when the song changes. */
function BlockWipe() {
  const path = useSongChange()
  if (!path) return null
  return (
    <div key={path} aria-hidden className="pointer-events-none fixed inset-x-0 bottom-0 z-[80] h-[124px] overflow-hidden" style={{ mixBlendMode: 'multiply' }}>
      <div className="fx-wipe absolute inset-0 flex">
        {['var(--block)', 'var(--accent-2)', 'var(--fix)', 'var(--block)'].map((c, i) => (
          <span key={i} className="h-full flex-1" style={{ background: c, opacity: 0.75 }} />
        ))}
      </div>
    </div>
  )
}

/** Thermoptic: faint columns of digits and katakana, drawn once, drifting down slowly. */
function DataRain({ still = false }: { still?: boolean }) {
  const tile = useMemo(() => {
    const c = document.createElement('canvas')
    c.width = 480
    c.height = 960
    const ctx = c.getContext('2d')
    if (!ctx) return ''
    const glyphs = '0123456789ｱｲｳｴｵｶｷｸｹｺｻｼｽｾｿﾀﾁﾂﾃﾄﾅﾆﾇﾈﾉ'
    ctx.font = '14px "Share Tech Mono", monospace'
    for (let x = 6; x < 480; x += 22) {
      let y = Math.random() * 300
      const len = 8 + Math.floor(Math.random() * 30)
      for (let k = 0; k < len && y < 960; k++, y += 18) {
        const fade = k / len
        ctx.fillStyle = `rgba(72,245,154,${(0.25 + 0.75 * fade).toFixed(2)})`
        ctx.fillText(glyphs[Math.floor(Math.random() * glyphs.length)], x, y)
      }
    }
    return c.toDataURL()
  }, [])
  return (
    <div
      className={`absolute inset-x-0 -top-[960px] h-[calc(100%+960px)] ${still ? '' : 'fx-rain'}`}
      style={{ backgroundImage: `url(${tile})`, backgroundSize: '480px 960px', opacity: 0.06 }}
    />
  )
}

/** Layer Zero: telephone wires across the top of the window. */
function Wires() {
  return (
    <svg className="absolute inset-x-0 top-0 h-[140px] w-full" viewBox="0 0 1000 140" preserveAspectRatio="none" aria-hidden>
      {[
        'M-20 30 Q 500 90 1020 22',
        'M-20 48 Q 480 112 1020 40',
        'M-20 64 Q 520 128 1020 60',
        'M-20 18 Q 300 52 640 -10',
      ].map((d) => (
        <path key={d} d={d} fill="none" stroke="#e4e7ec" strokeOpacity="0.07" strokeWidth="1" vectorEffect="non-scaling-stroke" />
      ))}
    </svg>
  )
}
