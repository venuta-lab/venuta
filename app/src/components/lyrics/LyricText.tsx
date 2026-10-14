import type { CSSProperties } from 'react'
import type { LookId } from '../../lib/lyricLook'

/** Words not yet sung, and lines around the one being sung. */
const DIM = 'rgb(255 255 255 / 0.42)'
const SHADOW = '0 2px 14px rgb(0 0 0 / 0.55), 0 1px 3px rgb(0 0 0 / 0.5)'

/**
 * A line filling in as it is sung: by letters, word after word, so a line that wraps fills its
 * first row and then its second, as it is read.
 */
function Sung(props: { text: string; progress: number; colour: string }) {
  const parts = props.text.split(/(\s+)/)
  const letters = props.text.replace(/\s+/g, '').length || 1
  let done = props.progress * letters
  return (
    <>
      {parts.map((w, i) => {
        if (!w.trim()) return w
        const share = Math.min(1, Math.max(0, done / w.length))
        done -= w.length
        const pct = Math.round(share * 1000) / 10
        return (
          <span
            key={i}
            style={{
              color: 'transparent',
              backgroundImage: `linear-gradient(90deg, ${props.colour} ${pct}%, ${DIM} ${pct}%)`,
              backgroundClip: 'text',
              WebkitBackgroundClip: 'text',
            }}
          >
            {w}
          </span>
        )
      })}
    </>
  )
}

/**
 * One line of lyrics in a look. `now` is the line being sung; `progress` (0 to 1) how far through
 * it the song is (the karaoke fill); `colour` the look's colour as CSS. `latin`, when given, is
 * the same line in Roman letters, shown under it.
 */
export function LyricText(props: {
  text: string
  latin?: string
  look: LookId
  now: boolean
  progress?: number
  colour: string
  /** Font size in px. */
  px: number
  align?: 'center' | 'left'
  className?: string
}) {
  const { look, now, colour } = props
  const p = Math.round((props.progress ?? (now ? 1 : 0)) * 1000) / 10
  const base: CSSProperties = {
    fontSize: props.px,
    lineHeight: 1.18,
    textAlign: props.align ?? 'center',
    textWrap: 'balance',
    letterSpacing: '-0.01em',
    fontWeight: 700,
    fontFamily: 'var(--font-ui)',
    color: now ? colour : DIM,
    textShadow: SHADOW,
    transition: 'color 300ms ease, opacity 300ms ease',
  }
  const style: CSSProperties = { ...base }
  switch (look) {
    case 'karaoke':
      // Filled word by word in reading order (below), however the line wraps.
      if (now) Object.assign(style, { color: DIM, textShadow: 'none', filter: 'drop-shadow(0 2px 8px rgb(0 0 0 / 0.55))', transition: 'none' })
      break
    case 'glow':
      if (now)
        style.textShadow = `0 0 18px ${colour}, 0 0 46px color-mix(in oklab, ${colour} 70%, transparent), 0 2px 4px rgb(0 0 0 / 0.5)`
      if (now) style.color = `color-mix(in oklab, ${colour} 35%, white)`
      break
    case 'outline':
      Object.assign(style, {
        color: now ? '#ffffff' : 'rgb(255 255 255 / 0.6)',
        WebkitTextStroke: `${Math.max(2, props.px / 16)}px ${now ? 'rgb(0 0 0 / 0.85)' : 'rgb(0 0 0 / 0.55)'}`,
        paintOrder: 'stroke fill',
        textShadow: now ? `0 0 0.5em color-mix(in oklab, ${colour} 55%, transparent)` : 'none',
      })
      break
    case 'serif':
      Object.assign(style, {
        fontFamily: '"Shippori Mincho B1", Georgia, "Times New Roman", serif',
        fontWeight: 800,
        letterSpacing: '0',
        lineHeight: 1.3,
      })
      break
    case 'theme':
      Object.assign(style, {
        fontFamily: 'var(--font-display)',
        fontWeight: 'var(--title-weight)' as unknown as number,
        letterSpacing: 'var(--title-tracking)',
        textTransform: 'var(--title-case)' as CSSProperties['textTransform'],
      })
      break
    case 'clean':
      break
  }
  return (
    <div className={props.className}>
      <div style={style}>{look === 'karaoke' && now && props.text ? <Sung text={props.text} progress={p / 100} colour={colour} /> : props.text || '♪'}</div>
      {props.latin && (
        <div
          style={{
            ...base,
            fontSize: props.px * 0.52,
            fontWeight: 500,
            marginTop: props.px * 0.22,
            color: now ? 'rgb(255 255 255 / 0.85)' : 'rgb(255 255 255 / 0.4)',
            WebkitTextStroke: look === 'outline' ? `${Math.max(1.5, props.px / 26)}px rgb(0 0 0 / 0.8)` : undefined,
            paintOrder: 'stroke fill',
          }}
        >
          {props.latin}
        </div>
      )}
    </div>
  )
}
